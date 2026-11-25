# Installs Bios from its newest GitHub release:
#   irm https://raw.githubusercontent.com/maxwraae/bios-native/main/install.ps1 | iex
#
# Into %LOCALAPPDATA%\Programs\bios, or over a copy already installed somewhere else.
#
# Why this line and not the zip from a browser: Windows marks a browser download with the Mark of
# the Web, and SmartScreen stops a marked program that is not signed by a paid-for certificate.
# `Invoke-WebRequest` does not mark what it downloads, and `Expand-Archive` of an unmarked zip
# writes unmarked files, so Bios opens with nothing in the way. `Unblock-File` below removes the
# mark anyway, for the case where someone downloaded the zip by hand and ran this afterwards.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = 'maxwraae/bios-native'
# `bios`, not `Bios`: craft-update's portable installer owns a folder only under this name, so a
# copy put here today can be replaced in place by a later version.
$leaf = 'bios'

# `$env:OS` and not `$IsWindows`: Windows PowerShell 5.1, which is what "PowerShell" opens to on
# a PC, has no `$IsWindows`, and `Set-StrictMode` turns reading a variable that does not exist
# into an error. So the obvious test would fail on the one version most people have.
if ($env:OS -ne 'Windows_NT') {
    throw 'install.ps1 installs Bios on Windows. On a Mac use install.sh.'
}

# One x64 build serves every PC: an Arm PC runs it too.
$pattern = 'bios-.*-windows-x64-portable\.zip$'

Write-Host 'Looking for the newest Bios'
$release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers @{ Accept = 'application/vnd.github+json' }
$asset = $release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1
if (-not $asset) {
    throw "No Bios release for Windows at https://github.com/$repo/releases"
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("bios-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zip = Join-Path $tmp $asset.name
    Write-Host "Downloading $($asset.name)"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip

    # The release publishes SHA256SUMS.txt beside the zip. A release without one is not installed:
    # saying nothing here would mean installing whatever arrived.
    $sumsAsset = $release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' } | Select-Object -First 1
    if (-not $sumsAsset) {
        throw "That release has no SHA256SUMS.txt, so the download cannot be checked; nothing was installed."
    }
    $sums = (Invoke-WebRequest -Uri $sumsAsset.browser_download_url -UseBasicParsing).Content
    $want = $null
    foreach ($line in ($sums -split "`r?`n")) {
        # "<hex>  <name>", with an optional * before the name, as shasum writes it.
        if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+)$' -and $Matches[2].Trim() -eq $asset.name) {
            $want = $Matches[1]
        }
    }
    if (-not $want) {
        throw "SHA256SUMS.txt does not list $($asset.name); nothing was installed."
    }
    $got = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash
    if ($got -ine $want) {
        throw "The download does not match the release's checksum; nothing was installed."
    }

    # Bios already installed is replaced where it is.
    $installed = Get-Command -Name 'bios-cli' -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty Source
    if ($installed) {
        $dest = Split-Path -Parent $installed
    } else {
        $dest = Join-Path $env:LOCALAPPDATA (Join-Path 'Programs' $leaf)
    }

    # Ask an open window to close before its program is replaced, and only insist if it will not.
    foreach ($p in @(Get-Process -Name 'bios' -ErrorAction SilentlyContinue)) {
        $null = $p.CloseMainWindow()
        if (-not $p.WaitForExit(5000)) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    }

    # Unpacked beside the destination first, so a half-written copy never replaces a working one.
    $staged = Join-Path $tmp 'staged'
    Expand-Archive -Path $zip -DestinationPath $staged -Force
    Get-ChildItem -Path $staged -Recurse -File | Unblock-File
    foreach ($program in @('bios.exe', 'bios-cli.exe')) {
        if (-not (Test-Path (Join-Path $staged $program))) {
            throw "The release's zip does not hold $program; nothing was installed."
        }
    }

    New-Item -ItemType Directory -Path $dest -Force | Out-Null
    Copy-Item -Path (Join-Path $staged '*') -Destination $dest -Recurse -Force

    Write-Host "Installed Bios in $dest"

    # Shown, not opened: this is a program list entry, not a window appearing by itself.
    $menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Bios.lnk'
    try {
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($menu)
        $link.TargetPath = Join-Path $dest 'bios.exe'
        $link.WorkingDirectory = $dest
        $link.Description = 'Bios: designing DNA'
        $link.Save()
    } catch {
        Write-Host 'Bios is installed; a Start Menu entry could not be made.'
    }

    # Proof, not an announcement: the installed program answers a question with a known answer.
    $cli = Join-Path $dest 'bios-cli.exe'
    # Windows PowerShell 5.1 strips the double quotes out of an argument on its way to a program
    # that is not PowerShell, so the JSON would arrive as {sequence:ATG...} and be refused.
    # Escaping them is what 5.1 needs and what 7 would pass through literally, so the two differ.
    $json = if ($PSVersionTable.PSVersion.Major -ge 7) {
        '{"sequence":"ATGGTGAGCAAGGGCGAGGAGCTG"}'
    } else {
        '{\"sequence\":\"ATGGTGAGCAAGGGCGAGGAGCTG\"}'
    }
    $said = & $cli run $env:TEMP sequence_stats $json 2>&1 | Out-String
    if ($said -notmatch '64\.7') {
        throw "Bios is installed in $dest but did not answer correctly, so something is wrong:`n$said"
    }
    Write-Host 'Checked: it reports a melting temperature of 64.7 C for a test sequence, which is right.'

    Write-Host ''

    # Connecting the agent is the half of the install that is easy to forget, so do it here when
    # Claude Code is on this machine rather than printing a line and hoping. `claude mcp add`
    # refuses a name it already has and leaves the old path in place, so an earlier `bios` is
    # removed first: whoever runs this installer means *this* copy to be the one the agent talks
    # to. Only the user scope is touched, so a project's shared settings are left alone.
    $claude = Get-Command -Name 'claude' -ErrorAction SilentlyContinue
    if ($claude) {
        # Guarded, because Bios is installed and working by this point and nothing here is worth
        # failing over: if Claude Code cannot be driven, the printed line is the way out. The
        # exit code is read inside the guard too — `claude` may be a shim rather than a program,
        # and under Set-StrictMode reading an exit code that was never set is itself an error.
        $connected = $false
        try {
            & $claude.Source mcp remove --scope user bios *> $null
            & $claude.Source mcp add --scope user bios -- "$cli" mcp *> $null
            $connected = ($LASTEXITCODE -eq 0)
        } catch {
            $connected = $false
        }
        if ($connected) {
            Write-Host 'Connected to Claude Code as "bios".'
            Write-Host 'Now type /mcp in Claude Code and reconnect "bios" - its tools appear only after that.'
        } else {
            Write-Host 'Bios is installed, but connecting it to Claude Code did not work. Run this yourself:'
            Write-Host "  claude mcp add --scope user bios -- `"$cli`" mcp"
        }
    } else {
        Write-Host 'Connect an agent (Claude Code):'
        Write-Host "  claude mcp add --scope user bios -- `"$cli`" mcp"
    }
}
finally {
    Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
