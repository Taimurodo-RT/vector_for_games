# Windows PowerShell 5.1 compatible. Dot-source to test without network access.
param([string]$InstallDir = $PSScriptRoot, [string]$Package = '')
$ErrorActionPreference = 'Stop'

function Get-MotifPath([string]$Root, [string]$Relative) {
    if (-not $Relative -or $Relative -match '(^[\\/]|:|(^|[\\/])\.\.([\\/]|$))') { throw "Unsafe package path: $Relative" }
    $basePath = [IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $path = [IO.Path]::GetFullPath((Join-Path $basePath $Relative))
    if (-not $path.StartsWith($basePath + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Path outside installation: $Relative" }
    $part = $path
    while ($part.Length -gt $basePath.Length) {
        if (Test-Path -LiteralPath $part) {
            if ((Get-Item -LiteralPath $part -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Linked installation path: $part" }
        }
        $part = Split-Path -Parent $part
    }
    return $path
}

function Remove-MotifTransaction([string]$Root) {
    $tx = Get-MotifPath $Root '.motif-update'
    if (Test-Path -LiteralPath $tx) {
        if (-not (Test-Path -LiteralPath "$tx/owned-by-motif" -PathType Leaf)) { throw "Unknown update directory: $tx" }
        Remove-Item -LiteralPath $tx -Recurse -Force
    }
}

function Write-MotifJournal([string]$Tx, $Journal) {
    [IO.File]::WriteAllText("$Tx/journal.tmp", ($Journal | ConvertTo-Json -Depth 8))
    if (Test-Path -LiteralPath "$Tx/journal.json") {
        [IO.File]::Replace("$Tx/journal.tmp", "$Tx/journal.json", "$Tx/journal.previous")
    } else { [IO.File]::Move("$Tx/journal.tmp", "$Tx/journal.json") }
}

function Restore-MotifTransaction([string]$Root) {
    $tx = Get-MotifPath $Root '.motif-update'
    if (-not (Test-Path -LiteralPath $tx)) { return }
    if (-not (Test-Path -LiteralPath "$tx/owned-by-motif" -PathType Leaf)) { throw "Unknown update directory: $tx" }
    if (Test-Path -LiteralPath "$tx/journal.json") {
        $journal = Get-Content -LiteralPath "$tx/journal.json" -Raw | ConvertFrom-Json
        if ($journal.state -ne 'committed') {
            foreach ($entry in $journal.files) {
                $dst = Get-MotifPath $Root $entry.path
                if ($entry.existed) {
                    $backup = Get-MotifPath "$tx/backup" $entry.path
                    if (-not (Test-Path -LiteralPath $backup -PathType Leaf)) { throw "Missing rollback file: $backup" }
                    if ((Test-Path -LiteralPath $dst -PathType Leaf) -and (Get-FileHash -LiteralPath $dst).Hash -eq (Get-FileHash -LiteralPath $backup).Hash) { continue }
                    Copy-Item -LiteralPath $backup -Destination $dst -Force
                } elseif (Test-Path -LiteralPath $dst -PathType Leaf) { Remove-Item -LiteralPath $dst -Force }
            }
            Write-Host 'Previous installation restored.'
        }
    }
    Remove-MotifTransaction $Root
}

function Read-MotifPackage([string]$Zip, [string]$Stage) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        $size = 0L
        foreach ($entry in $archive.Entries) {
            [void](Get-MotifPath $Stage $entry.FullName)
            if (($entry.ExternalAttributes -shr 16 -band 0xF000) -eq 0xA000) { throw 'Package contains a symbolic link.' }
            $size += $entry.Length
            if ($size -gt 2GB) { throw 'Package is too large.' }
        }
    } finally { $archive.Dispose() }
    [IO.Compression.ZipFile]::ExtractToDirectory($Zip, $Stage)
    $manifest = Get-Content -LiteralPath "$Stage/package-manifest.json" -Raw | ConvertFrom-Json
    if ($manifest.schema -ne 1 -or -not $manifest.version -or -not $manifest.files) { throw 'Invalid package manifest.' }
    $seen = @{}
    foreach ($entry in $manifest.files) {
        $rel = $entry.path.Replace('\', '/')
        if ($rel -notmatch '^(motif-editor\.exe|motif\.exe|README\.txt|update\.ps1|update\.bat|version\.txt|docs/.+|examples/.+|godot-apps/.+|godot-addon/.+)$') { throw "Unknown program component: $rel" }
        $file = Get-MotifPath $Stage $rel
        if ($seen.ContainsKey($file)) { throw "Duplicate package file: $rel" }
        $seen[$file] = $true
        if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.sha256) { throw "Package checksum mismatch: $rel" }
    }
    foreach ($required in 'motif.exe', 'motif-editor.exe', 'update.ps1', 'update.bat', 'README.txt', 'version.txt', 'docs/index.html', 'godot-addon/addons/motif/bin/motif_godot.dll', 'godot-addon/addons/motif/version.txt') {
        if (-not $seen.ContainsKey((Get-MotifPath $Stage $required))) { throw "Incomplete package: $required" }
    }
    if ((Get-Content -LiteralPath "$Stage/version.txt" -Raw).Trim() -ne $manifest.version -or
        (Get-Content -LiteralPath "$Stage/godot-addon/addons/motif/version.txt" -Raw).Trim() -ne $manifest.version) { throw 'Mixed program and Godot runtime versions.' }
    return $manifest
}

function Install-MotifPackage([string]$Root, [string]$Stage, $Manifest) {
    $tx = Get-MotifPath $Root '.motif-update'
    if (Test-Path -LiteralPath $tx) { throw 'Previous transaction must be recovered first.' }
    New-Item -ItemType Directory -Path "$tx/backup" -Force | Out-Null
    [IO.File]::WriteAllText("$tx/owned-by-motif", '1')
    try {
        $files = @()
        foreach ($rel in @($Manifest.files | ForEach-Object { $_.path }) + @('package-manifest.json')) {
            $dst = Get-MotifPath $Root $rel
            $exists = Test-Path -LiteralPath $dst
            # User-editable examples and the running BAT bootstrap are add-only.
            if ($exists -and $rel -match '^(examples[\\/]|godot-apps[\\/]|update\.bat$)') { continue }
            if ($exists -and -not (Test-Path -LiteralPath $dst -PathType Leaf)) { throw "A directory occupies a program file: $dst" }
            if ($exists) {
                $backup = Get-MotifPath "$tx/backup" $rel
                New-Item -ItemType Directory -Path (Split-Path $backup) -Force | Out-Null
                Copy-Item -LiteralPath $dst -Destination $backup
            }
            $files += [pscustomobject]@{ path = $rel; existed = $exists }
        }
        $journal = @{ state = 'applying'; files = $files }
        Write-MotifJournal $tx $journal
        foreach ($entry in $files) {
            $dst = Get-MotifPath $Root $entry.path
            New-Item -ItemType Directory -Path (Split-Path $dst) -Force | Out-Null
            Copy-Item -LiteralPath (Get-MotifPath $Stage $entry.path) -Destination $dst -Force
        }
        $journal.state = 'committed'
        Write-MotifJournal $tx $journal
    } catch {
        $reason = $_
        Restore-MotifTransaction $Root
        throw $reason
    }
    try { Remove-MotifTransaction $Root } catch { Write-Warning "Update complete; cleanup deferred: $_" }
}

function Invoke-MotifUpdate([string]$Root, [string]$Zip = '') {
    $Root = [IO.Path]::GetFullPath($Root)
    if (-not (Test-Path -LiteralPath $Root -PathType Container)) { throw "Installation folder not found: $Root" }
    $lockPath = Get-MotifPath $Root '.motif-update.lock'
    $lock = [IO.File]::Open($lockPath, 'OpenOrCreate', 'ReadWrite', 'None')
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("motif-update-" + [Guid]::NewGuid().ToString('N'))
    try {
        $running = Get-Process motif-editor, motif -ErrorAction SilentlyContinue | Where-Object { $_.Path -and (Split-Path $_.Path) -eq $Root }
        if ($running) { throw 'Close Motif in this folder, then run update.bat again.' }
        Restore-MotifTransaction $Root
        New-Item -ItemType Directory -Path $tmp | Out-Null
        $remoteVersion = $null
        if (-not $Zip) {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
            $ProgressPreference = 'SilentlyContinue'
            $base = 'https://github.com/Taimurodo-RT/vector_for_games/releases/latest/download'
            Write-Host 'Checking the latest GitHub build...'
            Invoke-WebRequest -UseBasicParsing "$base/version.txt" -OutFile "$tmp/version.txt"
            $remoteVersion = (Get-Content -LiteralPath "$tmp/version.txt" -Raw).Trim()
            # Keep a local development build until its changes reach the release.
            $pending = Get-MotifPath $Root '.motif-pending-commit'
            if (Test-Path -LiteralPath $pending) {
                $commit = (Get-Content -LiteralPath $pending -Raw).Trim()
                if ($commit -notmatch '^[0-9a-f]{40}$' -or $remoteVersion -notmatch '-([0-9a-f]{7,40})$') { throw 'Cannot compare development and published versions.' }
                $releaseCommit = $Matches[1]
                $compare = Invoke-RestMethod -Uri "https://api.github.com/repos/Taimurodo-RT/vector_for_games/compare/$commit...$releaseCommit" -Headers @{ 'User-Agent' = 'Motif-Updater' }
                if ($compare.status -notin @('ahead', 'identical')) {
                    Write-Host 'Your build includes changes awaiting publication. Keeping it until they reach the GitHub release.'
                    return
                }
            }
            $Zip = "$tmp/Motif.zip"
            Invoke-WebRequest -UseBasicParsing "$base/Motif-windows-x64.zip" -OutFile $Zip
        }
        $manifest = Read-MotifPackage $Zip "$tmp/new"
        if ($remoteVersion -and $remoteVersion -ne $manifest.version) { throw 'Release changed during download. Run update.bat again.' }
        Install-MotifPackage $Root "$tmp/new" $manifest
        if ($remoteVersion) {
            $pending = Get-MotifPath $Root '.motif-pending-commit'
            if (Test-Path -LiteralPath $pending) { Remove-Item -LiteralPath $pending -Force }
        }
        Write-Host "Motif updated: $($manifest.version)" -ForegroundColor Green
    } finally {
        $lock.Dispose()
        $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
        if ($tmp.StartsWith($tempRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $tmp)) { Remove-Item -LiteralPath $tmp -Recurse -Force }
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    try { Invoke-MotifUpdate $InstallDir $Package } catch { Write-Host "Update failed: $($_.Exception.Message)" -ForegroundColor Red; exit 1 }
}
