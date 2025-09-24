use std::{collections::BTreeMap, sync::Arc};

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Secret;
use kube::{
    Api, Client, Resource, ResourceExt,
    api::{ObjectMeta, Patch, PatchParams},
    config::KubeConfigOptions,
    runtime::{
        WatchStreamExt,
        reflector::{self, Store, store::Writer},
        watcher,
    },
};
use tokio::sync::Notify;
use tracing::{error, info};

pub mod crds;
use crds::*;

pub async fn run() -> anyhow::Result<()> {
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
                    info!("SyncSecret '{}' created or updated", o.name_any());
                    new_crd_notifier.notify_waiters();
                }
                Err(e) => error!("CRD watcher error: {}", e),
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
    let c = client.clone();
    tokio::spawn(async move {
        store
            .wait_until_ready()
            .await
            .expect("Writer dropped before ready");

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
                        for sync_crd in store.state().iter() {
                            process_match(sync_crd, &secret, &c).await;
                        }
                    }
                }
            }
        }
    });
}

async fn process_match(target: &SyncSecret, secret: &Secret, client: &Client) {
    if secret.name_any() == target.spec.secret.name
        && secret
            .namespace()
            .map(|n| n == target.spec.secret.namespace)
            .unwrap_or(false)
    {
        info!(
            "Secret '{}' found in namespace: '{}' replicating to destination namespaces: {:?}",
            secret.name_any(),
            target.spec.secret.namespace,
            target.spec.destination_namespaces,
        );

        for n in target.spec.destination_namespaces.iter() {
            let new_secret = Secret {
                metadata: ObjectMeta {
                    namespace: Some(n.to_owned()),
                    name: Some(secret.name_any()),
                    labels: Some(BTreeMap::from([(
                        "homerow.ca/source-namespace".to_owned(),
                        target.spec.secret.namespace.to_owned(),
                    )])),
                    owner_references: Some(vec![target.owner_ref(&()).unwrap()]),
                    ..ObjectMeta::default()
                },
                data: secret.data.clone(),
                string_data: secret.string_data.clone(),
                type_: secret.type_.clone(),
                immutable: secret.immutable,
            };

            let patch = Patch::Apply(new_secret);
            let params = PatchParams::apply("syncsecret.homerow.ca");

            let api = Api::<Secret>::namespaced(client.clone(), n);

            if let Err(e) = api.patch(&secret.name_any(), &params, &patch).await {
                error!("Error patching secret: {}", e);
            } else {
                info!(
                    "Successfully patched secret '{}' in namespace '{}'",
                    secret.name_any(),
                    n
                );
            }
        }
    }
}
