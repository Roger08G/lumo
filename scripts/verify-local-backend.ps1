$ErrorActionPreference = "Stop"

$cargoScript = Join-Path $PSScriptRoot "cargo-lumo.ps1"
$projectRoot = Split-Path -Parent $PSScriptRoot

$clientSecretReferences = & rg -n "LUMO_(API_PASSWORD|SERVER_MASTER_KEY)" `
    (Join-Path $projectRoot "mobile/src-tauri/build.rs") `
    (Join-Path $projectRoot "mobile/src-tauri/src") `
    (Join-Path $projectRoot "crates/lumo-runtime/src")
if ($LASTEXITCODE -eq 0) {
    $clientSecretReferences | Write-Error
    throw "Server secrets must never be referenced by client source code."
}
if ($LASTEXITCODE -ne 1) { exit $LASTEXITCODE }

& $cargoScript fmt --all "--" "--check"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $cargoScript check --workspace --all-targets --all-features --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $cargoScript clippy --workspace --all-targets --all-features --locked "--" "-D" "warnings"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $cargoScript test --workspace --all-targets --all-features --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $cargoScript build --workspace --all-targets --all-features --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $cargoScript build -p lumo-runtime --bins --release --features local-tools --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $cargoScript build -p lumo-api --release --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$binaryDirectory = "C:\.android\lumo-target\release"
foreach ($binary in @("lumo-controller", "lumo-controlled", "lumo-debug")) {
    & (Join-Path $binaryDirectory "$binary.exe") self-test
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host "Lumo local backend verification passed."
