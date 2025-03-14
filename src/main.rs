use std::{collections::HashMap, sync::Arc, time::Duration};

use clap::Parser;
use futures::{StreamExt, TryStreamExt, stream};
use k8s_openapi::api::core::v1::Secret;
use kube::{
    Api, Client, CustomResourceExt, ResourceExt,
    api::ListParams,
    config::KubeConfigOptions,
    runtime::{WatchStreamExt, reflector, watcher},
};
use kube_derive::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

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
#[kube(group = "homerow.ca", version = "v1", kind = "Foo", namespaced)]
struct FooSpec {
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

    let (reader, writer) = reflector::store::<Foo>();

    let secret_api = Api::<Secret>::all(client.clone());
    tokio::spawn(async move {
        reader.wait_until_ready().await.unwrap();

        let mut secret_watcher = watcher(secret_api, watcher::Config::default())
            .applied_objects()
            .default_backoff()
            .boxed();

        while let Some(secret) = secret_watcher
            .try_next()
            .await
            .expect("Unable to read from secret stream")
        {
            for target in reader.state().iter() {
                process_match(&target, &secret).await;
            }
        }
    });

    let crd_api = Api::<Foo>::all(client.clone());
    watcher(crd_api, watcher::Config::default())
        .reflect(writer)
        .applied_objects()
        .default_backoff()
        .boxed()
        .for_each(|res| {
            let api = Api::<Secret>::all(client.clone());
            async move {
                match res {
                    Ok(o) => {
                        println!("Found: {}", o.name_any());
                        for secret in api.list(&ListParams::default()).await.expect("Failed to list secrets") {
                            process_match(&o, &secret).await;
                        }
                    }
                    Err(e) => println!("watcher error: {}", e),
                }
            }
        })
        .await;

    Ok(())
}

async fn process_match(target: &Foo, secret: &Secret) {
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
            println!("{}", serde_yaml::to_string(&Foo::crd()).unwrap());
        }
        Args::Run => {
            run().await?;
        }
    };

    Ok(())
}
