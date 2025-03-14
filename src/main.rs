use clap::Parser;
use k8s_openapi::api::core::v1::Secret;
use kube::{api::ListParams, config::KubeConfigOptions, Api, Client, CustomResourceExt};
use serde::{Serialize, Deserialize};
use kube_derive::CustomResource;
use schemars::JsonSchema;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    Run,
    Crds
}

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(group = "homerow.ca", version = "v1", kind = "Foo", namespaced)]
struct FooSpec {
    info: String,
}

async fn run() -> anyhow::Result<()> {
    let options = KubeConfigOptions {
        context: None,
        cluster: None,
        user: None,
    };
    let config = kube::Config::from_kubeconfig(&options).await?;
    let client = Client::try_from(config)?;

    let api = Api::<Secret>::all(client);

    let secrets = api.list(&ListParams::default()).await?;
    for s in secrets {
        dbg!(s);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args {
        Args::Crds => {
            println!("{}", serde_yaml::to_string(&Foo::crd()).unwrap());
        },
        Args::Run => {
            run().await?;
        },
    };


    Ok(())
}
