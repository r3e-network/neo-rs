# Neo N3 v3.10.1 C# oracle fixtures

These generators record observable results from the immutable upstream
revisions used by the Rust differential tests:

- Neo.VM: `004cd6070a940405818d9357638277dd44407e2e`
- Neo: `d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d`

The checked-in fixtures add only declarative inputs and hardfork applicability
to the generated records. `verify-recorded.py` compares every recorded
`observed` object to a fresh generator run and rejects missing or extra cases.

Run from the repository root after checking out the revisions above:

```bash
dotnet run --project scripts/oracles/v3101/neo-vm/neo-vm-oracle.csproj \
  --configuration Release -p:NeoVmSource=/path/to/neo-vm \
  > /tmp/neo-vm-v3101-oracle.json

dotnet run --project scripts/oracles/v3101/neo-application/neo-application-oracle.csproj \
  --configuration Release -p:NeoSource=/path/to/neo \
  > /tmp/neo-application-v3101-oracle.json

python3 scripts/oracles/v3101/verify-recorded.py \
  neo-vm/tests/fixtures/csharp-v3.10.1-vm.json \
  /tmp/neo-vm-v3101-oracle.json

python3 scripts/oracles/v3101/verify-recorded.py \
  neo-execution/tests/fixtures/csharp-v3.10.1-application.json \
  /tmp/neo-application-v3101-oracle.json
```

Do not update the recorded results from a moving branch or an unreviewed
revision. Rust production code never executes these C# projects.
