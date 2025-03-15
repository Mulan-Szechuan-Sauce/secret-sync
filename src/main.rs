use std::sync::Arc;

use clap::Parser;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Secret;
use kube::{
    Api, Client, CustomResourceExt, ResourceExt,
    config::KubeConfigOptions,
    runtime::{
        WatchStreamExt,
        reflector::{self, Store, store::Writer},
        watcher,
    },
};
use kube_derive::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    Run,
    Crds,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
struct SecretTarget {
    name: String,
    namespace: String,
}

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(group = "homerow.ca", version = "v1", kind = "SyncSecret")]
struct SyncSecretSpec {
    secret: SecretTarget,
}

async fn run() -> anyhow::Result<()> {
    let options = KubeConfigOptions::default();

    // Load kubeconfig if it's present otherwise fall back to cluster config
    let config = kube::Config::from_kubeconfig(&options)
        .await
        .or_else(|_| kube::Config::incluster())?;
    let client = Client::try_from(config)?;

    let (reader, writer) = reflector::store::<SyncSecret>();
    let new_crd_notifier = Arc::new(Notify::new());

    spawn_secret_watcher_with_interrupt(&client, reader, new_crd_notifier.clone());

    watch_crds(&client, writer, new_crd_notifier).await;

    Ok(())
}

async fn watch_crds(client: &Client, writer: Writer<SyncSecret>, new_crd_notifier: Arc<Notify>) {
    let crd_api = Api::<SyncSecret>::all(client.clone());
    watcher(crd_api, watcher::Config::default())
        .reflect(writer)
        .applied_objects()
        .default_backoff()
        .boxed()
        .for_each(|res| async {
            match res {
                Ok(o) => {
                    println!("Found: {}", o.name_any());
                    new_crd_notifier.notify_waiters();
                }
                Err(e) => println!("watcher error: {}", e),
            }
        })
        .await;
}

fn spawn_secret_watcher_with_interrupt(
    client: &Client,
    store: Store<SyncSecret>,
    new_crd_notifier: Arc<Notify>,
) {
    let secret_api = Api::<Secret>::all(client.clone());
    tokio::spawn(async move {
        store.wait_until_ready().await.expect("Writer dropped before ready");

        loop {
            let mut secret_watcher = watcher(secret_api.clone(), watcher::Config::default())
                .applied_objects()
                .default_backoff()
                .boxed();

            // If we get a new CRD in the store recreate the secret watcher and reprocess
            // everything otherwise wait for secrets to be changed and process as they come
            loop {
                tokio::select! {
                    _ = new_crd_notifier.notified() => break,

                    Ok(Some(secret)) = secret_watcher.try_next() => {
                        for target in store.state().iter() {
                            process_match(&target, &secret).await;
                        }
                    }
                }
            }
        }
    });
}

async fn process_match(target: &SyncSecret, secret: &Secret) {
    if secret.name_any() == target.spec.secret.name
        && secret
            .namespace()
            .map(|n| n == target.spec.secret.namespace)
            .unwrap_or(false)
    {
        println!(
            "Matching secret: {} found in namespace: {}",
            secret.name_any(),
            target.spec.secret.namespace
        );
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args {
        Args::Crds => {
            println!("{}", serde_yaml::to_string(&SyncSecret::crd()).unwrap());
        }
        Args::Run => {
            run().await?;
        }
    };

    Ok(())
}
