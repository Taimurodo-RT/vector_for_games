# Обновляет Motif в этой папке до последней сборки с GitHub.
# Запускается из update.bat. Свои сцены (.msc) и другие файлы в папке не трогает.

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'

$dir = $PSScriptRoot
$base = 'https://github.com/Taimurodo-RT/vector_for_games/releases/latest/download'
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("motif-update-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $tmp | Out-Null

try {
    $versionFile = Join-Path $dir 'version.txt'
    $current = if (Test-Path $versionFile) { (Get-Content $versionFile -Raw).Trim() } else { 'неизвестна' }
    Write-Host "Установлена версия: $current"
    Write-Host 'Проверяю последнюю сборку на GitHub...'
    Invoke-WebRequest -UseBasicParsing "$base/version.txt" -OutFile "$tmp\version.txt"
    $latest = (Get-Content "$tmp\version.txt" -Raw).Trim()
    Write-Host "Последняя версия:   $latest"
    # Папка godot-apps появилась в сборке позже: без неё качаем сборку и при той же версии.
    $apps = Join-Path $dir 'godot-apps'
    if ($latest -eq $current -and (Test-Path $apps)) {
        Write-Host ''
        Write-Host 'У вас уже последняя версия.' -ForegroundColor Green
        return
    }

    while (Get-Process motif-editor -ErrorAction SilentlyContinue) {
        Write-Host ''
        Write-Host 'Редактор Motif сейчас открыт. Сохраните сцену, закройте редактор и нажмите Enter.' -ForegroundColor Yellow
        [void](Read-Host)
    }

    Write-Host 'Скачиваю сборку...'
    Invoke-WebRequest -UseBasicParsing "$base/Motif-windows-x64.zip" -OutFile "$tmp\motif.zip"
    Expand-Archive "$tmp\motif.zip" -DestinationPath "$tmp\new" -Force

    # Программа и справка заменяются целиком.
    foreach ($name in 'motif-editor.exe', 'motif.exe', 'README.txt', 'update.ps1') {
        $src = Join-Path "$tmp\new" $name
        if (Test-Path $src) { Copy-Item $src (Join-Path $dir $name) -Force }
    }
    # Документация заменяется целиком.
    if (Test-Path "$tmp\new\docs") {
        Remove-Item (Join-Path $dir 'docs') -Recurse -Force -ErrorAction SilentlyContinue
        Copy-Item "$tmp\new\docs" (Join-Path $dir 'docs') -Recurse
    }
    # update.bat сейчас выполняется, поэтому его заменяем, только если его нет.
    if (-not (Test-Path (Join-Path $dir 'update.bat'))) { Copy-Item "$tmp\new\update.bat" $dir }
    # Примеры и приложения Godot: новые файлы добавляются, изменённые вами остаются как есть.
    foreach ($name in 'examples', 'godot-apps') {
        if (-not (Test-Path "$tmp\new\$name")) { continue }
        $into = Join-Path $dir $name
        New-Item -ItemType Directory -Force $into | Out-Null
        $src = (Resolve-Path "$tmp\new\$name").Path
        foreach ($f in Get-ChildItem $src -File -Recurse -ErrorAction SilentlyContinue) {
            $dst = Join-Path $into $f.FullName.Substring($src.Length + 1)
            New-Item -ItemType Directory -Force (Split-Path $dst) | Out-Null
            if (-not (Test-Path $dst)) { Copy-Item $f.FullName $dst }
        }
    }
    Copy-Item "$tmp\version.txt" $versionFile -Force

    Write-Host ''
    Write-Host "Готово: Motif обновлён до $latest." -ForegroundColor Green
}
catch {
    Write-Host ''
    Write-Host "Не удалось обновить: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host 'Проверьте интернет и попробуйте ещё раз. Файлы в папке не изменены, если ошибка случилась до копирования.'
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
