<#
.SYNOPSIS
    Compile OpenGear (release) depuis ce dépôt et lance la version fraîchement compilée.

.DESCRIPTION
    1. cargo build --release (dans le dossier target du dépôt, ou -TargetDir).
    2. Copie l'exécutable, WebView2Loader.dll et pawnio_modules dans target\dev-run.
    3. Arrête l'instance OpenGear en cours (OpenGear n'accepte qu'une instance) sans toucher au
       processus de surveillance des ventilateurs, qui rend la main au BIOS avant de se fermer.
    4. Lance la nouvelle version depuis target\dev-run.

    L'exécutable tourne depuis target\dev-run, pas depuis le dossier de build : la compilation
    suivante n'est donc jamais bloquée par le fichier en cours d'utilisation.

.PARAMETER Admin
    Lance OpenGear en administrateur (ventilateurs, RAM RGB, capteurs avancés).

.PARAMETER BuildOnly
    Compile et prépare target\dev-run sans arrêter ni lancer OpenGear.

.PARAMETER TargetDir
    Dossier target de cargo (par défaut : <dépôt>\target).
#>
param(
    [switch]$Admin,
    [switch]$BuildOnly,
    [string]$TargetDir
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (-not $TargetDir) { $TargetDir = Join-Path $repo 'target' }
$release = Join-Path $TargetDir 'x86_64-pc-windows-gnullvm\release'
$runDir = Join-Path $repo 'target\dev-run'
$Host.UI.RawUI.WindowTitle = 'OpenGear - compilation et lancement'

function Fail([string]$message) {
    Write-Host ""
    Write-Host $message -ForegroundColor Red
    Read-Host 'Appuie sur Entrée pour fermer'
    exit 1
}

# Main OpenGear processes only: the fan watchdog (--fan-watchdog) must survive so it can hand
# the fans back to the BIOS once the main instance is gone.
function Get-MainInstances {
    Get-CimInstance Win32_Process -Filter "Name = 'opengear.exe'" |
        Where-Object { $_.CommandLine -notmatch '--fan-watchdog' }
}

function Stop-OpenGear {
    $main = @(Get-MainInstances)
    if ($main.Count -eq 0) { return }
    Write-Host "Arrêt de l'instance OpenGear en cours ($($main.ProcessId -join ', '))..."
    foreach ($p in $main) {
        try {
            Stop-Process -Id $p.ProcessId -ErrorAction Stop
        } catch {
            Fail ("Impossible d'arrêter OpenGear (PID $($p.ProcessId)) : il tourne probablement en " +
                "administrateur. Quitte-le depuis la zone de notification (clic droit > Quitter) " +
                "puis relance ce raccourci.")
        }
    }
    # Wait for every OpenGear process (watchdog included) to release the files in target\dev-run.
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline) {
        $left = @(Get-CimInstance Win32_Process -Filter "Name = 'opengear.exe'" |
            Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($runDir, 'OrdinalIgnoreCase') })
        if ($left.Count -eq 0 -and @(Get-MainInstances).Count -eq 0) { return }
        Start-Sleep -Milliseconds 300
    }
    Fail "OpenGear ne s'est pas arrêté à temps. Réessaie dans quelques secondes."
}

Set-Location $repo
$branch = (git rev-parse --abbrev-ref HEAD) 2>$null
$commit = (git log -1 --format='%h %s') 2>$null
Write-Host "Dépôt   : $repo"
Write-Host "Branche : $branch"
Write-Host "Commit  : $commit"
$dirty = (git status --porcelain) 2>$null
if ($dirty) { Write-Host 'Modifications locales non commitées incluses dans le build.' -ForegroundColor Yellow }
Write-Host ''

Write-Host 'Compilation (release)...' -ForegroundColor Cyan
cargo build --release --locked --target-dir "$TargetDir"
if ($LASTEXITCODE -ne 0) {
    # First run after launching OpenGear straight from target\...\release: the exe is locked.
    if (-not $BuildOnly -and @(Get-MainInstances).Count -gt 0) {
        Write-Host "Échec, peut-être un exécutable verrouillé : arrêt d'OpenGear puis nouvel essai." -ForegroundColor Yellow
        Stop-OpenGear
        cargo build --release --locked --target-dir "$TargetDir"
    }
    if ($LASTEXITCODE -ne 0) { Fail 'La compilation a échoué (voir les erreurs ci-dessus).' }
}

if (-not $BuildOnly) { Stop-OpenGear }

Write-Host "Préparation de $runDir..."
New-Item -ItemType Directory -Force -Path (Join-Path $runDir 'pawnio_modules') | Out-Null
Copy-Item (Join-Path $release 'opengear.exe') $runDir -Force
Copy-Item (Join-Path $release 'WebView2Loader.dll') $runDir -Force
Copy-Item (Join-Path $release 'pawnio_modules\*') (Join-Path $runDir 'pawnio_modules') -Force
$ico = Join-Path $release 'opengear.ico'
if (Test-Path $ico) { Copy-Item $ico $runDir -Force }

$exe = Join-Path $runDir 'opengear.exe'
if ($BuildOnly) {
    Write-Host "Prêt : $exe" -ForegroundColor Green
    exit 0
}

Write-Host 'Lancement d''OpenGear...' -ForegroundColor Green
if ($Admin) {
    Start-Process -FilePath $exe -WorkingDirectory $runDir -Verb RunAs
} else {
    Start-Process -FilePath $exe -WorkingDirectory $runDir
}
Start-Sleep -Seconds 2
