use std::time::Duration;

use k8s_openapi::{
    api::core::v1::{Namespace, Secret},
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
};
use kube::{
    Api, Client, CustomResourceExt, ResourceExt,
    api::{DeleteParams, ObjectMeta, Patch, PatchParams},
    config::KubeConfigOptions,
};
use secret_sync::crds::*;
use tokio::{sync::OnceCell, time::sleep};
use tokio_retry2::{Retry, RetryError, strategy::ExponentialBackoff};

static ONCE_CLIENT: OnceCell<anyhow::Result<Client>> = OnceCell::const_new();

async fn init() -> &'static Client {
    async fn init_fun() -> anyhow::Result<Client> {
        tracing_subscriber::fmt().init();

        let options = KubeConfigOptions::default();
        let config = kube::Config::from_kubeconfig(&options).await?;

        let cluster_host = config.cluster_url.host().expect("Cluster URL has host");
        if !["localhost", "127.0.0.1"].contains(&cluster_host) {
            panic!("Danger! Cluster URL isn't local.");
        }

        let client = Client::try_from(config)?;

        sleep(Duration::from_secs(5)).await;

        let crd_api = kube::Api::<CustomResourceDefinition>::all(client.clone());

        crd_api
            .patch(
                "syncsecrets.homerow.ca",
                &PatchParams::apply("test"),
                &Patch::Apply(SyncSecret::crd()),
            )
            .await?;

        tokio::spawn(secret_sync::run());

        Ok(client)
    }

    ONCE_CLIENT.get_or_init(init_fun).await.as_ref().unwrap()
}

async fn setup_manifests() -> (SyncSecret, Secret) {
    let client = init().await;

    let ns_api = kube::Api::<Namespace>::all(client.clone());
    for ns in ["ns-one", "ns-two"] {
        ns_api
            .patch(
                ns,
                &PatchParams::apply("test"),
                &Patch::Apply(Namespace {
                    metadata: ObjectMeta {
                        name: Some(ns.to_owned()),
                        ..ObjectMeta::default()
                    },
                    ..Namespace::default()
                }),
            )
            .await
            .expect("Failed to create namespace");
    }

    let sync_api = kube::Api::<SyncSecret>::all(client.clone());

    let syncer = serde_yaml::from_str::<SyncSecret>(include_str!("./manifests/syncer.yaml"))
        .expect("Unable to deserialize SyncSecret manifest");

    sync_api
        .patch(
            &syncer.name_any(),
            &PatchParams::apply("test"),
            &Patch::Apply(syncer.clone()),
        )
        .await
        .expect("Unable to patch SyncSecret");

    let secret_api = kube::Api::<Secret>::namespaced(client.clone(), &syncer.spec.secret.namespace);
    let secret = serde_yaml::from_str::<Secret>(include_str!("./manifests/secret.yaml"))
        .expect("Unable to deserialize Secret manifest");

    secret_api
        .patch(
            &secret.name_any(),
            &PatchParams::apply("test"),
            &Patch::Apply(secret.clone()),
        )
        .await
        .expect("Unable to patch Secret");

    (syncer, secret)
}

#[tokio::test]
async fn secrets_replicate() {
    let client = init().await;

    let (syncer, secret) = setup_manifests().await;

    let retry_strategy = ExponentialBackoff::from_millis(10)
        .map(tokio_retry2::strategy::jitter)
        .take(3);

    Retry::spawn(
        retry_strategy.clone(),
        async || -> Result<(), RetryError<()>> {
            for target in syncer.spec.destination_namespaces.iter() {
                let secret_api = Api::<Secret>::namespaced(client.clone(), target);

                validate_secret_replicated(&secret_api, &secret).await?;
            }

            Ok(())
        },
    )
    .await
    .expect("Failure replicating secrets");

    delete_syncer(client, &syncer.name_any()).await;

    Retry::spawn(
        retry_strategy.clone(),
        async || -> Result<(), RetryError<()>> {
            for target in syncer.spec.destination_namespaces.iter() {
                let secret_api = Api::<Secret>::namespaced(client.clone(), target);

                validate_secret_deleted(&secret_api, &secret).await?;
            }

            Ok(())
        },
    )
    .await
    .expect("Owned secrets not successfully deleted");
}

async fn delete_syncer(client: &Client, name: &str) {
    let sync_api = kube::Api::<SyncSecret>::all(client.clone());
    sync_api
        .delete(name, &DeleteParams::foreground())
        .await
        .expect("Unable to delete SyncSecret");
}

async fn validate_secret_replicated(
    secret_api: &Api<Secret>,
    secret: &Secret,
) -> Result<(), RetryError<()>> {
    match secret_api.get(&secret.name_any()).await {
        Ok(s) => {
            assert_eq!(s.data, secret.data);
            assert_eq!(s.string_data, secret.string_data);
            assert_eq!(s.type_, secret.type_);
            assert_eq!(s.immutable, secret.immutable);
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            return Err(RetryError::transient(()));
        }
        Err(e) => panic!("Unable to get secret: {}", e),
    }
    Ok(())
}

async fn validate_secret_deleted(
    secret_api: &Api<Secret>,
    secret: &Secret,
) -> Result<(), RetryError<()>> {
    match secret_api.get(&secret.name_any()).await {
        Ok(_) => Err(RetryError::transient(())),
        Err(kube::Error::Api(e)) if e.code == 404 => Ok(()),
        Err(e) => panic!("Unable to get secret: {}", e),
    }
}
