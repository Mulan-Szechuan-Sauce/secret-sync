use std::sync::Arc;

use clap::Parser;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Secret;
use kube::{
    config::KubeConfigOptions, runtime::{reflector, watcher, WatchStreamExt}, Api, Client, CustomResourceExt, ResourceExt
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
    let options = KubeConfigOptions {
        context: None,
        cluster: None,
        user: None,
    };
    let config = kube::Config::from_kubeconfig(&options).await?;
    let client = Client::try_from(config)?;

    let (reader, writer) = reflector::store::<SyncSecret>();

    let new_crd_notify = Arc::new(Notify::new());
    let new_crd_awaiter = new_crd_notify.clone();

    let secret_api = Api::<Secret>::all(client.clone());
    tokio::spawn(async move {
        reader.wait_until_ready().await.unwrap();

        loop {
            let mut secret_watcher = watcher(secret_api.clone(), watcher::Config::default())
                .applied_objects()
                .default_backoff()
                .boxed();

            loop {
                tokio::select! {
                    _ = new_crd_awaiter.notified() => break,

                    Ok(Some(secret)) = secret_watcher.try_next() => {
                        for target in reader.state().iter() {
                            process_match(&target, &secret).await;
                        }
                    }
                }
            }
        }
    });

    let crd_api = Api::<SyncSecret>::all(client.clone());
    watcher(crd_api, watcher::Config::default())
        .reflect(writer)
        .applied_objects()
        .default_backoff()
        .boxed()
        .for_each(|res| {
            let notify = new_crd_notify.clone();
            async move {
                match res {
                    Ok(o) => {
                        println!("Found: {}", o.name_any());
                        notify.notify_waiters();
                    }
                    Err(e) => println!("watcher error: {}", e),
                }
            }
        })
        .await;

    Ok(())
}

async fn process_match(target: &SyncSecret, secret: &Secret) {
    if secret.name_any() == target.spec.secret.name
        && secret.namespace().expect("Secrets should have namespaces")
            == target.spec.secret.namespace
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
