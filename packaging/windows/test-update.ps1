# Integration tests: actual ZIPs, filesystem locks and interrupted transaction recovery.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/update.ps1"
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("motif-updater-test-" + [Guid]::NewGuid().ToString('N'))
function Assert($Value, [string]$Message) { if (-not $Value) { throw $Message } }
function Put([string]$Path, [string]$Text) {
    New-Item -ItemType Directory -Path (Split-Path $Path) -Force | Out-Null
    [IO.File]::WriteAllText($Path, $Text)
}
function MustFail([scriptblock]$Action, [string]$Pattern) {
    try { & $Action } catch {
        if ($_.Exception.Message -notmatch $Pattern) { throw }
        return
    }
    throw "Expected failure: $Pattern"
}
try {
    $source = "$testRoot/package"
    $dest = "$testRoot/installation with spaces"
    New-Item -ItemType Directory -Path $dest -Force | Out-Null
    foreach ($rel in 'motif.exe', 'motif-editor.exe', 'update.ps1', 'update.bat', 'README.txt', 'version.txt', 'docs/index.html', 'godot-addon/addons/motif/bin/motif_godot.dll', 'godot-addon/addons/motif/version.txt', 'examples/test.msc', 'godot-apps/test.msc') { Put "$source/$rel" 'new' }
    $files = @(Get-ChildItem $source -File -Recurse | Sort-Object FullName | ForEach-Object {
        @{ path = $_.FullName.Substring($source.Length + 1).Replace('\', '/'); sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash }
    })
    @{ schema = 1; version = 'new'; files = $files } | ConvertTo-Json -Depth 5 | Set-Content "$source/package-manifest.json" -Encoding UTF8
    Compress-Archive "$source/*" "$testRoot/good.zip"
    $manifest = Read-MotifPackage "$testRoot/good.zip" "$testRoot/stage"
    foreach ($rel in 'motif.exe', 'motif-editor.exe', 'README.txt', 'version.txt', 'examples/test.msc', 'godot-apps/test.msc', 'my-scene.msc') { Put "$dest/$rel" 'original' }
    Invoke-MotifUpdate $dest "$testRoot/good.zip"
    Assert ((Get-Content "$dest/motif.exe" -Raw) -eq 'new') 'Program was not updated'
    foreach ($rel in 'examples/test.msc', 'godot-apps/test.msc', 'my-scene.msc') { Assert ((Get-Content "$dest/$rel" -Raw) -eq 'original') "User file changed: $rel" }
    Assert (-not (Test-Path "$dest/.motif-update")) 'Successful transaction was not cleaned'
    Write-Host 'PASS: installation preserves custom files and edited examples'

    Put "$source/motif.exe" 'corrupt'
    Compress-Archive "$source/*" "$testRoot/corrupt.zip"
    MustFail { Invoke-MotifUpdate $dest "$testRoot/corrupt.zip" } 'checksum mismatch'
    Assert ((Get-Content "$dest/motif.exe" -Raw) -eq 'new') 'Corrupt package changed installation'
    Write-Host 'PASS: corrupted download changes no installed file'

    $incomplete = @{ schema=1; version='new'; files=@($files | Where-Object { $_.path -ne 'motif.exe' }) }
    $incomplete | ConvertTo-Json -Depth 5 | Set-Content "$source/package-manifest.json" -Encoding UTF8
    Compress-Archive "$source/*" "$testRoot/incomplete.zip"
    MustFail { Invoke-MotifUpdate $dest "$testRoot/incomplete.zip" } 'Incomplete package'
    $zipAttack = [IO.Compression.ZipFile]::Open("$testRoot/traversal.zip", 'Create')
    try { [void]$zipAttack.CreateEntry('../outside.txt') } finally { $zipAttack.Dispose() }
    MustFail { Invoke-MotifUpdate $dest "$testRoot/traversal.zip" } 'Unsafe package path'
    Write-Host 'PASS: missing components and traversal ZIP rejected before installation'

    # A late write failure must roll back earlier successful writes, including additions.
    Put "$dest/docs/index.html" 'previous docs'
    Put "$dest/README.txt" 'previous readme'
    $locked = [IO.File]::Open("$dest/README.txt", 'Open', 'Read', 'Read')
    try { MustFail { Install-MotifPackage $dest "$testRoot/stage" $manifest } 'process|being used|access' } finally { $locked.Dispose() }
    Assert ((Get-Content "$dest/docs/index.html" -Raw) -eq 'previous docs') 'Earlier write did not roll back'
    Assert ((Get-Content "$dest/README.txt" -Raw) -eq 'previous readme') 'Locked file changed'
    Assert (-not (Test-Path "$dest/.motif-update")) 'Rollback did not clean transaction'
    Write-Host 'PASS: real locked-file failure rolls back earlier writes'

    $tx = "$dest/.motif-update"
    Put "$tx/owned-by-motif" '1'
    Put "$tx/backup/motif.exe" 'before interrupted update'
    Put "$dest/motif.exe" 'partial update'
    Put "$dest/docs/added.html" 'partial new file'
    Write-MotifJournal $tx @{ state = 'applying'; files = @(@{path='motif.exe';existed=$true}, @{path='docs/added.html';existed=$false}) }
    Restore-MotifTransaction $dest
    Assert ((Get-Content "$dest/motif.exe" -Raw) -eq 'before interrupted update') 'Interrupted update not restored'
    Assert (-not (Test-Path "$dest/docs/added.html")) 'Partial addition not removed'
    Write-Host 'PASS: interrupted update is recoverable on next run'

    foreach ($bad in '../outside', '/outside', 'C:/outside', 'docs/../../outside') { MustFail { Get-MotifPath $dest $bad } 'Unsafe|outside' }
    $lock = [IO.File]::Open("$dest/.motif-update.lock", 'OpenOrCreate', 'ReadWrite', 'None')
    try { MustFail { Invoke-MotifUpdate $dest "$testRoot/good.zip" } 'process|being used|access' } finally { $lock.Dispose() }
    Write-Host 'PASS: traversal paths and concurrent updater are rejected'
    Write-Host 'All updater integration tests passed.'
} catch {
    Write-Host ($_ | Out-String)
    throw
} finally {
    if ($locked) { $locked.Dispose() }
    if ($lock) { $lock.Dispose() }
    $tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
    if ($testRoot.StartsWith($tempBase + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $testRoot)) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
