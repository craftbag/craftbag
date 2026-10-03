$ErrorActionPreference = 'Stop'
$packageName = $env:ChocolateyPackageName
$toolsDir = "$(Split-Path -Parent $MyInvocation.MyCommand.Definition)"
$url64 = 'https://github.com/craftbag/craftbag/releases/download/v0.2.0/craftbag-x86_64-pc-windows-msvc.zip'
$checksum64 = 'e3fa8856e021b17551f9dada72c81e0dc99f6b70258b4577562f25c259fc5f3f'
$packageArgs = @{
    packageName    = $packageName
    unzipLocation  = $toolsDir
    url64bit       = $url64
    checksum64     = $checksum64
    checksumType64 = 'sha256'
}
Install-ChocolateyZipPackage @packageArgs
