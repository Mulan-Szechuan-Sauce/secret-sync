# Secret Sync

## Installation

```bash
helm repo add secret-sync https://mulan-szechuan-sauce.github.io/secret-sync/
helm install secret-sync secret-sync/secret-sync
```

## Usage

Then you can create manifests that look like:

```yaml
apiVersion: homerow.ca/v1
kind: SyncSecret
metadata:
  name: test-syncer
spec:
  secret:
    name: some-secret
    namespace: default
  destinationNamespaces:
    - ns-one
    - ns-two
```

This will watch a secret named `some-secret` and replicate it to namespaces `ns-one` and `ns-two`.

## Local Development

<details>

To generate and apply the CRDs run:

```bash
cargo run -- crds > crds.yaml
kubectl apply -f crds.yaml
```

Then to actually run the app itself run:


```bash
cargo run -- run
```

If desired you can create the test secret and namespaces with:

```bash
kubectl create namespace ns-one
kubectl create namespace ns-two
kubectl create secret generic some-secret --from-literal=somekey=somevalue
```

</details>
