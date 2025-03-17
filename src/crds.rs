use kube_derive::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct SecretTarget {
    pub name: String,
    pub namespace: String,
}

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[kube(group = "homerow.ca", version = "v1", kind = "SyncSecret")]
pub struct SyncSecretSpec {
    pub secret: SecretTarget,
    pub destination_namespaces: Vec<String>,
}


