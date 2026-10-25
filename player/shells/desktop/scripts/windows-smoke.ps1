# Дымовая проверка установщика на настоящей Windows (её запускает
# .github/workflows/windows-installer.yml, можно и вручную):
# тихая установка в профиль пользователя, ярлык, ассоциации плейлистов,
# запуск, двойной щелчок по плейлисту при открытом плеере, удаление.
#
#   pwsh scripts/windows-smoke.ps1 -Installer '...\Scrobb Player_0.1.0_x64-setup.exe' [-Screenshot shot.png]
#
# Внимание: ставит и удаляет Scrobb Player у текущего пользователя.
# Нужен Python 3 (читает user.db).
param(
    [Parameter(Mandatory)] [string] $Installer,
    [string] $Screenshot
)
$ErrorActionPreference = 'Stop'

function Check([bool] $ok, [string] $what) {
    if (-not $ok) { throw "Не прошло: $what" }
    Write-Host "ok: $what"
}

function Wait-Until([scriptblock] $condition, [int] $seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $condition) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return [bool] (& $condition)
}

function Default-Value([string] $key) {
    (Get-Item -LiteralPath $key).GetValue('')
}

function Player-Processes {
    @(Get-Process -Name 'scrobb-player' -ErrorAction SilentlyContinue)
}

$dir = Join-Path $env:LOCALAPPDATA 'Scrobb Player'
$exe = Join-Path $dir 'scrobb-player.exe'
$userDb = Join-Path $env:APPDATA 'ru.scrobb.player\user.db'
$classes = 'HKCU:\Software\Classes'

# 1. Установка: без прав администратора, в %LOCALAPPDATA%.
$setup = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
Check ($setup.ExitCode -eq 0) "установщик завершился с кодом 0 (код $($setup.ExitCode))"
Check (Test-Path -LiteralPath $exe) "программа в $dir"
Check (Test-Path -LiteralPath (Join-Path $dir 'uninstall.exe')) 'есть uninstall.exe'
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Scrobb Player.lnk'
Check (Test-Path -LiteralPath $startMenu) 'ярлык в меню «Пуск»'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Scrobb Player'
Check (Test-Path -LiteralPath $uninstallKey) 'запись в «Установленных приложениях»'

# 2. Ассоциации: только плейлисты, команда с путём в кавычках.
$expected = @{ m3u8 = 'M3U'; m3u = 'M3U'; pls = 'PLS'; xspf = 'XSPF'; jspf = 'JSPF' }
foreach ($ext in $expected.Keys) {
    $class = "ScrobbPlayer.$($expected[$ext])"
    Check ((Default-Value "$classes\.$ext") -eq $class) ".$ext → $class"
    $command = Default-Value "$classes\$class\shell\open\command"
    Check ($command -eq "`"$exe`" `"%1`"") "команда открытия $class`: $command"
}
foreach ($ext in 'mp3', 'flac') {
    $class = if (Test-Path -LiteralPath "$classes\.$ext") { Default-Value "$classes\.$ext" } else { '' }
    Check (-not "$class".StartsWith('ScrobbPlayer.')) ".$ext не тронут"
}

# 3. Запуск: окно живёт, ядро создало базу правок.
$first = Start-Process -FilePath $exe -PassThru
Check (Wait-Until { Test-Path -LiteralPath $userDb } 30) "создана $userDb"
Start-Sleep -Seconds 5
Check (-not $first.HasExited) 'плеер запущен и не упал'

# 4. Двойной щелчок по плейлисту (через ассоциацию): файл уходит в открытое окно.
$work = Join-Path ([IO.Path]::GetTempPath()) 'scrobb-smoke'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$title = 'Дымовой тест'
$playlist = Join-Path $work "$title.m3u8"
[IO.File]::WriteAllText($playlist, "#EXTM3U`n#EXTINF:215,Кино - Кукушка`nКино - Кукушка.flac`n", [Text.UTF8Encoding]::new($false))
Invoke-Item -LiteralPath $playlist
$query = @'
import sqlite3, sys
titles = [r[0] for r in sqlite3.connect(sys.argv[1]).execute('select title from playlists')]
sys.exit(0 if sys.argv[2] in titles else 1)
'@
Check (Wait-Until { python -c $query $userDb $title; $LASTEXITCODE -eq 0 } 30) "плейлист «$title» импортирован"
Start-Sleep -Seconds 3
Check ((Player-Processes).Count -eq 1) 'второй запуск отдал файл первому окну и вышел'
Check (-not $first.HasExited) 'первое окно по-прежнему открыто'

if ($Screenshot) {
    try {
        Add-Type -AssemblyName System.Windows.Forms, System.Drawing
        $screen = [Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bitmap = [Drawing.Bitmap]::new($screen.Width, $screen.Height)
        [Drawing.Graphics]::FromImage($bitmap).CopyFromScreen($screen.Location, [Drawing.Point]::Empty, $screen.Size)
        $bitmap.Save($Screenshot, [Drawing.Imaging.ImageFormat]::Png)
        Write-Host "снимок экрана: $Screenshot"
    } catch {
        Write-Warning "снимок экрана не вышел: $_"
    }
}

# 5. Выход и удаление. Деинсталлятор NSIS перезапускает себя из TEMP —
# ждём, пока исчезнет программа.
$first.CloseMainWindow() | Out-Null
if (-not (Wait-Until { $first.HasExited } 15)) { Stop-Process -Id $first.Id -Force }
Start-Process -FilePath (Join-Path $dir 'uninstall.exe') -ArgumentList '/S' -Wait
Check (Wait-Until { -not (Test-Path -LiteralPath $exe) } 60) 'программа удалена'
Check (-not (Test-Path -LiteralPath $startMenu)) 'ярлык удалён'
Check (-not (Test-Path -LiteralPath $uninstallKey)) 'запись об установке удалена'
Check (-not (Test-Path -LiteralPath "$classes\ScrobbPlayer.M3U")) 'класс ScrobbPlayer.M3U удалён'
Check (Test-Path -LiteralPath $userDb) 'данные пользователя остались (плейлисты, история)'
Write-Host 'Дымовая проверка пройдена.'
