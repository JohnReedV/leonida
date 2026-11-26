# The program resolves graphics settings, including profiles, leonida.toml and explicit
# environment overrides. Query the same plan for launch messages and measurement reports;
# never copy profile values into this PowerShell window.
function Get-LeonidaGraphics {
    [CmdletBinding()]
    param([string]$Exe)
    if (-not $Exe) { throw 'no leonida executable was supplied for graphics settings' }
    $errors = [System.IO.Path]::GetTempFileName()
    try {
        # Windows PowerShell 5.1 represents native stderr as errors. Keep it separate from
        # the JSON and check the native exit code ourselves, including when stderr is used.
        $before = $ErrorActionPreference
        $encodingBefore = [Console]::OutputEncoding
        $ErrorActionPreference = 'Continue'
        $PSNativeCommandUseErrorActionPreference = $false
        try {
            # Rust writes UTF-8; Windows PowerShell otherwise decodes the console code page.
            [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
            $out = & $Exe config --graphics 2> $errors
            $code = $LASTEXITCODE
        } catch {
            throw "could not query graphics settings from '$Exe': $_"
        } finally {
            [Console]::OutputEncoding = $encodingBefore
            $ErrorActionPreference = $before
        }
        $detail = (Get-Content -LiteralPath $errors -Raw -ErrorAction SilentlyContinue)
        if ($code -ne 0) {
            throw ("leonida config --graphics failed (exit $code); " +
                "check leonida.toml and graphics profiles. $detail")
        }
        try {
            $plan = ($out -join "`n") | ConvertFrom-Json -ErrorAction Stop
            foreach ($field in 'gpu', 'profiles', 'settings', 'sources') {
                if ($field -notin $plan.PSObject.Properties.Name) { throw "missing '$field'" }
            }
            if ($plan.settings -isnot [pscustomobject] -or $plan.sources -isnot [pscustomobject]) {
                throw 'settings and sources must be JSON objects'
            }
        } catch {
            throw ("leonida config --graphics returned invalid JSON settings; " +
                "rebuild leonida.exe. $_")
        }
        if ($detail) { Write-Warning $detail.Trim() }
        $plan
    } finally {
        Remove-Item -LiteralPath $errors -ErrorAction SilentlyContinue
    }
}

function Show-LeonidaGraphics($Plan) {
    $gpu = $Plan.gpu
    $card = if ($gpu) { '{0} ({1}, {2} MB)' -f $gpu.name, $gpu.vendor, $gpu.vram_mb }
        else { 'unknown' }
    $profiles = if (@($Plan.profiles).Count) { $Plan.profiles -join ', ' } else { 'none' }
    Write-Host ("graphics card: {0}; profiles: {1}" -f $card, $profiles)
    $kept = @($Plan.sources.PSObject.Properties | Where-Object { $_.Value -eq 'environment' } |
        Sort-Object Name | ForEach-Object { $_.Name })
    if ($kept.Count) { Write-Host ("explicit environment overrides: {0}" -f ($kept -join ', ')) }
}

# Shareable reports show simple switch values only, never paths, hashes or arbitrary text.
# Keep the complete values local in `leonida config --graphics`.
function Format-LeonidaGraphicsSwitch([string]$Name, [string]$Value, [switch]$Short) {
    if ($Name -notmatch '\ALEONIDA_[A-Z0-9_]+\z') { return '<unknown switch>=<set>' }
    $simple = '\A(?:[0-9]{1,10}|0x[0-9a-f]{1,8}|alt|on|off|<set>|<empty>)\z'
    $safe = if ($Value -match $simple) { $Value }
        elseif ($Value -eq '') { '<empty>' }
        else { '<set>' }
    if ($Short) { $Name = $Name -replace '^LEONIDA_', '' }
    '{0}={1}' -f $Name, $safe
}

# Includes native-only settings and sanitizes values for messages and shareable reports.
function Get-LeonidaGraphicsSwitches($Plan, [switch]$Short) {
    $Plan.settings.PSObject.Properties | Sort-Object Name | ForEach-Object {
        Format-LeonidaGraphicsSwitch $_.Name $_.Value -Short:$Short
    }
}

# Launchers change process environment variables, which otherwise survive into another
# script in the same PowerShell window. Restore the caller's values even after a failure.
function Save-LeonidaEnvironment {
    $saved = @{}
    # .NET Framework's single-variable getter (also used by Env:) can mistake an
    # existing empty value for a missing one after a stale Win32 error. The full
    # environment block preserves both presence and value without that lookup.
    foreach ($entry in [Environment]::GetEnvironmentVariables().GetEnumerator()) {
        if ($entry.Key -like 'LEONIDA_*') { $saved[$entry.Key] = $entry.Value }
    }
    $saved
}

# .NET Framework and older PowerShell versions delete empty values through Env:.
# The Windows API distinguishes an explicit empty override from an absent variable.
function Set-LeonidaEmptyEnvironment([string]$Name) {
    if (-not ('LeonidaLauncherEnvironment' -as [type])) {
        Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class LeonidaLauncherEnvironment {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetEnvironmentVariable(string name, string value);
}
'@
    }
    if (-not [LeonidaLauncherEnvironment]::SetEnvironmentVariable($Name, '')) {
        throw "could not restore empty environment value $Name"
    }
}

function Restore-LeonidaEnvironment([System.Collections.IDictionary]$Before) {
    $current = Save-LeonidaEnvironment
    foreach ($name in $current.Keys) {
        if (-not $Before.Contains($name)) { [Environment]::SetEnvironmentVariable($name, $null) }
    }
    foreach ($name in $Before.Keys) {
        if (-not $current.Contains($name) -or $current[$name] -cne $Before[$name]) {
            if ($Before[$name] -eq '' -and
                [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
                Set-LeonidaEmptyEnvironment $name
            } else {
                [Environment]::SetEnvironmentVariable($name, $Before[$name])
            }
        }
    }
}

# A measurement baseline disables both profiles and TOML graphics settings. Explicit
# runtime overrides must also go; paths, logging and early startup controls remain available.
function Disable-LeonidaGraphics {
    $stay = 'LEONIDA_CONFIG', 'LEONIDA_GAME_DIR', 'LEONIDA_CACHE_DIR', 'LEONIDA_DATA_DIR',
        'LEONIDA_TEST_TITLES', 'LEONIDA_PROFILE', 'LEONIDA_LOG', 'LEONIDA_KEEP_WINDOW',
        'LEONIDA_GRAPHICS_OFF', 'LEONIDA_D3D12_WARP', 'LEONIDA_RELAUNCHED_FROM',
        'LEONIDA_AUDIO_CAPTURE_OF'
    $current = Save-LeonidaEnvironment
    foreach ($name in $current.Keys) {
        if ($name -notin $stay -and $name -notlike 'LEONIDA_RENDERDOC*') {
            [Environment]::SetEnvironmentVariable($name, $null)
        }
    }
    $env:LEONIDA_GRAPHICS_OFF = '1'
}
