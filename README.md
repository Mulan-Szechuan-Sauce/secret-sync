# Secret Sync

## Development Usage

```bash
cargo run -- crds > crds.yaml
kubectl apply -f crds.yaml

cargo run -- run
```

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

If desired you can create the secret and namespaces with:

```bash
kubectl create namespace ns-one
kubectl create namespace ns-two
kubectl create secret generic some-secret --from-literal=somekey=somevalue
```
