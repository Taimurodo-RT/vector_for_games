# Collects the Windows build into a folder and a zip: what update.ps1 downloads.
# Usage: package.ps1 -Out Motif -Version 2026.10.25-abc1234
param([string]$Out = 'Motif', [string]$Version = 'local', [string]$BinaryDir = '', [string]$Archive = 'Motif-windows-x64.zip')
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Out) { throw 'Use a new empty output folder for each package.' }
$root = Resolve-Path "$PSScriptRoot/../.."
if (-not $BinaryDir) {
    $BinaryDir = if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'release' } else { "$root/target/release" }
}
New-Item -ItemType Directory -Force "$Out/examples" | Out-Null
Copy-Item "$BinaryDir/motif-editor.exe", "$BinaryDir/motif.exe" $Out
Copy-Item "$root/examples/*.msc" "$Out/examples/"
# Templates of game screens behind «Файл → Новое…».
Copy-Item "$root/examples/screens" "$Out/examples/screens" -Recurse -Force
# Fonts the examples use, with their licences.
Copy-Item "$root/examples/fonts" "$Out/examples/fonts" -Recurse -Force
Copy-Item "$root/docs" "$Out/docs" -Recurse -Force
# Godot apps whose interface is drawn in Motif: projects to open in Godot.
Copy-Item "$root/godot-apps" "$Out/godot-apps" -Recurse -Force
# The Godot addon (MotifSprite), with its library, on its own and inside the hero example.
$addon = "$Out/godot-addon/addons/motif"
New-Item -ItemType Directory -Force "$Out/godot-addon/addons" | Out-Null
Copy-Item "$root/godot-addon/addons/motif" $addon -Recurse -Force
New-Item -ItemType Directory -Force "$addon/bin" | Out-Null
Copy-Item "$BinaryDir/motif_godot.dll" "$addon/bin/"
# The build the addon comes from: Motif tells an old addon from a new one by it.
Set-Content -Path "$addon/version.txt" -Value $Version -NoNewline -Encoding ascii
New-Item -ItemType Directory -Force "$Out/godot-apps/hero/addons" | Out-Null
Copy-Item $addon "$Out/godot-apps/hero/addons/motif" -Recurse -Force
Copy-Item "$PSScriptRoot/README.txt", "$PSScriptRoot/update.bat", "$PSScriptRoot/update.ps1" $Out
Set-Content -Path "$Out/version.txt" -Value $Version -NoNewline -Encoding ascii
$outRoot = (Resolve-Path -LiteralPath $Out).Path
$files = @(Get-ChildItem -LiteralPath $outRoot -File -Recurse | Sort-Object FullName | ForEach-Object {
    @{ path = $_.FullName.Substring($outRoot.Length + 1).Replace('\', '/'); sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
})
@{ schema = 1; version = $Version; files = $files } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath "$Out/package-manifest.json" -Encoding UTF8
Compress-Archive -Path "$Out/*" -DestinationPath $Archive -Force
