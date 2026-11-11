param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference = 'Stop'
$file = Get-Item -LiteralPath $Executable
if ($file.Name -cnotmatch '^Claude-Account-Switcher-(\d+\.\d+\.\d+)-windows-x64\.exe$') {
    throw 'Expected the versioned Windows x64 release filename.'
}
$version = $Matches[1]
if ((& $file.FullName --version | Out-String).Trim() -ne $version) { throw 'Executable version does not match its filename.' }
$asset = @{
    name = $file.Name
    state = 'uploaded'
    size = $file.Length
    digest = 'sha256:' + (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    browser_download_url = "https://github.com/yuukana1/claude-account-switcher/releases/download/v$version/$($file.Name)"
}
$manifest = @{ tag_name = "v$version"; draft = $false; prerelease = $false; assets = @($asset) }
[IO.File]::WriteAllText((Join-Path $file.DirectoryName 'update.json'), ($manifest | ConvertTo-Json -Depth 4), [Text.UTF8Encoding]::new($false))
