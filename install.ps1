# Installe swarm-control sous Windows (dernière version publiée sur GitHub).
#
#   irm https://raw.githubusercontent.com/DeamonDev888/swarm-control/main/install.ps1 | iex
#
# - Télécharge l'exécutable dans %LOCALAPPDATA%\Programs\swarm-control
#   (ou dans le dossier donné par la variable SWARM_INSTALL_DIR).
# - Ajoute ce dossier au PATH de l'utilisateur (aucun droit administrateur requis).
# - Relancer le script met à jour l'exécutable.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest est bien plus rapide sans barre
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repo = 'DeamonDev888/swarm-control'
$asset = 'swarm-control-windows-x86_64.exe'
$url = "https://github.com/$repo/releases/latest/download/$asset"

if (-not [Environment]::Is64BitOperatingSystem) {
    Write-Host "swarm-control n'est publié que pour Windows 64 bits." -ForegroundColor Red
    return
}

$dir = $env:SWARM_INSTALL_DIR
if (-not $dir) { $dir = Join-Path $env:LOCALAPPDATA 'Programs\swarm-control' }
$exe = Join-Path $dir 'swarm-control.exe'

Write-Host "-> Téléchargement de $asset"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$tmp = "$exe.download"
try {
    Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
} catch {
    Write-Host "Échec du téléchargement depuis $url" -ForegroundColor Red
    Write-Host "Téléchargez l'exécutable à la main : https://github.com/$repo/releases/latest"
    throw
}
Move-Item -Force -Path $tmp -Destination $exe
Unblock-File -Path $exe -ErrorAction SilentlyContinue   # évite l'avertissement « fichier venu d'Internet »

# PATH utilisateur (persistant) + session courante.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$entries = @()
if ($userPath) { $entries = $userPath -split ';' | Where-Object { $_ } }
if ($entries -notcontains $dir) {
    [Environment]::SetEnvironmentVariable('Path', (($entries + $dir) -join ';'), 'User')
    Write-Host "-> $dir ajouté à votre PATH"
}
if (($env:Path -split ';') -notcontains $dir) { $env:Path = "$env:Path;$dir" }

$version = & $exe --version
Write-Host ""
Write-Host "OK  $version installé dans $exe" -ForegroundColor Green
Write-Host ""
Write-Host "Ensuite :"
Write-Host "  swarm-control doctor     # vérifie que tout est prêt"
Write-Host "  swarm-control            # démarre le serveur"
Write-Host ""
Write-Host "(Ouvrez un nouveau terminal si la commande n'est pas trouvée.)"
