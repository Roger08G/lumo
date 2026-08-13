$ErrorActionPreference = "Stop"

$cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
if ($null -eq $cargoCommand) {
    $fallbackCargo = "C:\.android\cargo\bin\cargo.exe"
    if (-not (Test-Path -LiteralPath $fallbackCargo -PathType Leaf)) {
        throw "Cargo is not installed or available in PATH."
    }
    $env:RUSTUP_HOME = "C:\.android\rustup"
    $env:CARGO_HOME = "C:\.android\cargo"
    $env:CARGO_TARGET_DIR = "C:\.android\lumo-target"
    $env:Path = "C:\.android\cargo\bin;$env:Path"
    $cargoExecutable = $fallbackCargo
}
else {
    $cargoExecutable = $cargoCommand.Source
}

if ($env:OS -eq "Windows_NT" -and $null -eq (Get-Command link.exe -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere -PathType Leaf) {
        $installationPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        $developerCommand = Join-Path $installationPath "Common7\Tools\VsDevCmd.bat"
        if (Test-Path -LiteralPath $developerCommand -PathType Leaf) {
            $environmentCommand = "`"$developerCommand`" -no_logo -arch=x64 -host_arch=x64 >nul && set"
            foreach ($line in (& $env:ComSpec /d /s /c $environmentCommand)) {
                $separator = $line.IndexOf("=")
                if ($separator -gt 0) {
                    [Environment]::SetEnvironmentVariable(
                        $line.Substring(0, $separator),
                        $line.Substring($separator + 1),
                        "Process"
                    )
                }
            }
        }
    }
}

$cargoArguments = @($args)
& $cargoExecutable @cargoArguments
exit $LASTEXITCODE
