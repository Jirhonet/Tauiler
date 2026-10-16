$ErrorActionPreference = 'Stop'
$repoPath = Split-Path -Parent $PSScriptRoot
Push-Location $repoPath
try {
    cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting failed' }
    cargo test --workspace --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    cargo clippy --workspace --locked --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
    cargo build --workspace --locked --release
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    cargo clippy -p tauiler-creation-hook --target i686-pc-windows-msvc --locked --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw '32-bit hook Clippy failed' }
    cargo test -p tauiler-creation-hook --target i686-pc-windows-msvc --locked
    if ($LASTEXITCODE -ne 0) { throw '32-bit hook tests failed' }
    cargo build -p tauiler-creation-hook --target i686-pc-windows-msvc --locked --release
    if ($LASTEXITCODE -ne 0) { throw '32-bit hook build failed; install the i686-pc-windows-msvc Rust target and MSVC x86 libraries' }
    $releasePath = Join-Path $repoPath 'target/release'
    $x86Path = Join-Path $repoPath 'target/i686-pc-windows-msvc/release'
    Copy-Item -LiteralPath (Join-Path $x86Path 'tauiler_creation_hook.dll') -Destination (Join-Path $releasePath 'tauiler_creation_hook32.dll')
    Copy-Item -LiteralPath (Join-Path $x86Path 'tauiler-hook-host.exe') -Destination (Join-Path $releasePath 'tauiler-hook-host32.exe')
    Copy-Item -LiteralPath (Join-Path $x86Path 'tauiler-hook-probe.exe') -Destination (Join-Path $releasePath 'tauiler-hook-probe32.exe')
    $executablePath = Join-Path $repoPath 'target/release/tauiler.exe'
    $testRun = Start-Process -FilePath $executablePath -ArgumentList '--self-test' -Wait -PassThru -WindowStyle Hidden
    if ($testRun.ExitCode -ne 0) { throw "Native self-test failed ($($testRun.ExitCode)); see %APPDATA%\Tauiler\tauiler.log" }
    $distributionPath = Join-Path $repoPath 'dist'
    New-Item -ItemType Directory -Path $distributionPath -Force | Out-Null
    Copy-Item -LiteralPath $executablePath -Destination (Join-Path $distributionPath 'tauiler.exe')
    $hookFiles = @('tauiler_creation_hook.dll', 'tauiler_creation_hook32.dll', 'tauiler-hook-host32.exe', 'tauiler-hook-probe.exe', 'tauiler-hook-probe32.exe')
    foreach ($name in $hookFiles) {
        $sourceFile = Join-Path $releasePath $name
        $destinationFile = Join-Path $distributionPath $name
        # Windows may retain a hook DLL reference after unhooking. Identical
        # binaries do not need replacing while applications release that reference.
        if (-not (Test-Path -LiteralPath $destinationFile) -or
            (Get-FileHash -LiteralPath $sourceFile).Hash -ne (Get-FileHash -LiteralPath $destinationFile).Hash) {
            Copy-Item -LiteralPath $sourceFile -Destination $destinationFile
        }
    }
    foreach ($name in @('README.md', 'config.example.toml', 'LICENSE')) {
        Copy-Item -LiteralPath (Join-Path $repoPath $name) -Destination (Join-Path $distributionPath $name)
    }
    $packageFiles = (@('tauiler.exe', 'README.md', 'config.example.toml', 'LICENSE') + $hookFiles) | ForEach-Object { Join-Path $distributionPath $_ }
    Compress-Archive -LiteralPath $packageFiles -DestinationPath (Join-Path $distributionPath 'tauiler-0.1.0-windows-x64.zip') -Force
    Write-Output "Built and verified: $distributionPath\tauiler.exe"
} finally {
    Pop-Location
}
