#Requires -Version 5.1
<#
.SYNOPSIS
  Download and run the latest BT KeepAlive setup.
.DESCRIPTION
  Fetches BTKeepAlive-setup.exe from the latest GitHub Release and runs it.
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

function Get-LatestAsset([string]$namePattern) {
  $api = "https://api.github.com/repos/$Repo/releases/latest"
  $release = Invoke-RestMethod -Uri $api -UseBasicParsing
  $asset = $release.assets | Where-Object { $_.name -like $namePattern } | Select-Object -First 1
  if (-not $asset) { throw "No asset matching $namePattern in $($release.tag_name)" }
  return $asset
}

if ($Portable) {
  $asset = Get-LatestAsset 'BTKeepAlive-portable.zip'
  $zip = Join-Path $env:TEMP $asset.name
  Write-Host "Download $($asset.name) from $($asset.browser_download_url)"
  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
  New-Item -ItemType Directory -Force -Path $Dest | Out-Null
  Expand-Archive -Path $zip -DestinationPath $Dest -Force
  Write-Host "Files ready in $Dest. Run BTKeepAlive.exe from that folder."
  return
}

$asset = Get-LatestAsset 'BTKeepAlive-setup.exe'
$setup = Join-Path $env:TEMP $asset.name
Write-Host "Download $($asset.name) from $($asset.browser_download_url)"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $setup -UseBasicParsing
Write-Host "Run $setup"
Start-Process -FilePath $setup
