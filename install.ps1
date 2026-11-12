# Install lucid-dream for the current user. No admin needed.
#   irm https://github.com/jksim26/lucid-dream/releases/latest/download/install.ps1 | iex
# Downloads the Windows binary, checks it against the release's SHA-256
# checksums, and puts it in %LOCALAPPDATA%\Programs\lucid-dream.
$ErrorActionPreference = "Stop"

$base  = "https://github.com/jksim26/lucid-dream/releases/latest/download"
$asset = "lucid-dream-windows-x64.exe"
$dir   = if ($env:LUCID_INSTALL_DIR) { $env:LUCID_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\lucid-dream" }
$tmp   = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    Write-Host "Downloading $asset"
    Invoke-WebRequest "$base/$asset" -OutFile (Join-Path $tmp $asset) -UseBasicParsing
    Invoke-WebRequest "$base/SHA256SUMS" -OutFile (Join-Path $tmp "SHA256SUMS") -UseBasicParsing

    $line = Get-Content (Join-Path $tmp "SHA256SUMS") | Where-Object { $_ -match " $([regex]::Escape($asset))$" }
    $want = if ($line) { ($line -split " ")[0] } else { "" }
    $got  = (Get-FileHash (Join-Path $tmp $asset) -Algorithm SHA256).Hash.ToLower()
    if (-not $want -or $want -ne $got) {
        throw "Checksum does not match; nothing was installed."
    }

    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    Move-Item (Join-Path $tmp $asset) (Join-Path $dir "lucid-dream.exe") -Force
    Write-Host "Installed $(& (Join-Path $dir 'lucid-dream.exe') --version) to $dir"

    $path = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($path -split ";") -notcontains $dir) {
        [Environment]::SetEnvironmentVariable("Path", "$path;$dir", "User")
        Write-Host "Added $dir to your user PATH; open a new terminal to use it."
    }
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
