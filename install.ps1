#Requires -Version 5.1
<#
.SYNOPSIS
  Download and run the latest BT KeepAlive release.
.DESCRIPTION
  Fetches BTKeepAlive-setup.exe from the latest GitHub Release and runs it.
  Falls back to BTKeepAlive.exe when the setup is not published yet, as with v1.4.4.
  Run without flags for the installer. Use -Portable to fetch the zip instead.
.EXAMPLE
  irm https://raw.githubusercontent.com/kadato/bt-keepalive/main/install.ps1 | iex
.EXAMPLE
  .\install.ps1 -Portable -Dest "$env:USERPROFILE\Apps\BTKeepAlive"
#>
[CmdletBinding()]
param(
  [switch]$Portable,
  [string]$Dest = "$env:USERPROFILE\Apps\BTKeepAlive"
)

$ErrorActionPreference = 'Stop'
$Repo = 'kadato/bt-keepalive'

function Get-LatestRelease {
  $api = "https://api.github.com/repos/$Repo/releases/latest"
  return Invoke-RestMethod -Uri $api -UseBasicParsing
}

function Find-Asset($release, [string]$namePattern) {
  return $release.assets | Where-Object { $_.name -like $namePattern } | Select-Object -First 1
}

function Install-SingleExe($asset, [string]$targetDir) {
  $out = Join-Path $targetDir 'BTKeepAlive.exe'
  New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
  Write-Host "Download $($asset.name) from $($asset.browser_download_url)"
  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $out -UseBasicParsing
  Write-Host "File ready in $targetDir. Run BTKeepAlive.exe from that folder."
  return $out
}

function Get-AssetNames($release) {
  return ($release.assets | ForEach-Object { $_.name }) -join ', '
}

$release = Get-LatestRelease

if ($Portable) {
  $asset = Find-Asset $release 'BTKeepAlive-portable.zip'
  if ($asset) {
    $zip = Join-Path $env:TEMP $asset.name
    Write-Host "Download $($asset.name) from $($asset.browser_download_url)"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
    New-Item -ItemType Directory -Force -Path $Dest | Out-Null
    Expand-Archive -Path $zip -DestinationPath $Dest -Force
    Write-Host "Files ready in $Dest. Run BTKeepAlive.exe from that folder."
    return
  }
  $exe = Find-Asset $release 'BTKeepAlive.exe'
  if ($exe) {
    Install-SingleExe $exe $Dest | Out-Null
    return
  }
  throw "No portable asset in $($release.tag_name). Found: $(Get-AssetNames $release)"
}

$asset = Find-Asset $release 'BTKeepAlive-setup.exe'
if ($asset) {
  $setup = Join-Path $env:TEMP $asset.name
  Write-Host "Download $($asset.name) from $($asset.browser_download_url)"
  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $setup -UseBasicParsing
  Write-Host "Run $setup"
  Start-Process -FilePath $setup
  return
}

$exe = Find-Asset $release 'BTKeepAlive.exe'
if ($exe) {
  $out = Install-SingleExe $exe $Dest
  Write-Host "Run $out"
  Start-Process -FilePath $out
  return
}

throw "No installer asset in $($release.tag_name). Found: $(Get-AssetNames $release)"
