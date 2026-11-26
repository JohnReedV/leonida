# No game files needed. Tests native delegation and measurement baseline handling.
# Supply a built executable to also exercise its real config --graphics JSON interface:
#   powershell -File scripts\test-graphics-settings.ps1 -Exe target\release\leonida.exe
param([string]$Exe = '')

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'leonida-settings.ps1')

function Assert-Graphics($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-GraphicsThrows([scriptblock]$Action, [string]$Pattern) {
    try { & $Action | Out-Null }
    catch {
        Assert-Graphics ("$_" -match $Pattern) "unexpected error: $_"
        return
    }
    throw "expected an error matching: $Pattern"
}

# This command stands in for the native executable. It deliberately provides profile and
# TOML values absent from Env:, so reading only the PowerShell environment cannot pass.
function Invoke-SyntheticLeonida {
    Assert-Graphics (($args -join ' ') -eq 'config --graphics') 'wrong native command'
    Set-Variable -Name LASTEXITCODE -Value 0 -Scope 1
    if ($script:queryMode -eq 'invalid') { 'not JSON'; return }
    if ($script:queryMode -eq 'failure') {
        Write-Error 'synthetic configuration error' -ErrorAction Continue
        Set-Variable -Name LASTEXITCODE -Value 19 -Scope 1
        return
    }
    if ($script:queryMode -eq 'stderr') {
        Write-Error 'synthetic native warning' -ErrorAction Continue
    }
    $settings = [ordered]@{}
    $sources = [ordered]@{}
    $profiles = @()
    if ($env:LEONIDA_GRAPHICS_OFF -ne '1') {
        $profiles = @('base', 'synthetic')
        $settings['LEONIDA_COPY_PLAIN'] = '1'
        $sources['LEONIDA_COPY_PLAIN'] = 'profile:base'
        $settings['LEONIDA_JOIN_PARTS'] = '0'
        $sources['LEONIDA_JOIN_PARTS'] = 'config'
    }
    if (Test-Path Env:LEONIDA_ASYNC_SUBMIT) {
        $settings['LEONIDA_ASYNC_SUBMIT'] = $env:LEONIDA_ASYNC_SUBMIT
        $sources['LEONIDA_ASYNC_SUBMIT'] = 'environment'
    }
    [ordered]@{ gpu = $null; profiles = $profiles; settings = $settings; sources = $sources } |
        ConvertTo-Json -Depth 4
}

$saved = Save-LeonidaEnvironment
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('leonida-graphics-test-' + [guid]::NewGuid())
try {
    Restore-LeonidaEnvironment @{}
    $script:queryMode = 'valid'
    $env:LEONIDA_ASYNC_SUBMIT = '0'
    $encodingBefore = [Console]::OutputEncoding.CodePage
    $plan = Get-LeonidaGraphics 'Invoke-SyntheticLeonida'
    Assert-Graphics ([Console]::OutputEncoding.CodePage -eq $encodingBefore) `
        'native query changed the caller console encoding'
    Assert-Graphics ($plan.profiles -join ',' -eq 'base,synthetic') 'native profiles were lost'
    Assert-Graphics ($env:LEONIDA_ASYNC_SUBMIT -eq '0') 'explicit override was changed'
    Assert-Graphics (-not (Test-Path Env:LEONIDA_COPY_PLAIN)) 'profile leaked into the shell'
    $switches = @(Get-LeonidaGraphicsSwitches $plan)
    Assert-Graphics ($switches -contains 'LEONIDA_COPY_PLAIN=1') 'profile missing from report'
    Assert-Graphics ($switches -contains 'LEONIDA_JOIN_PARTS=0') 'TOML setting missing from report'
    Assert-Graphics ($switches -contains 'LEONIDA_ASYNC_SUBMIT=0') 'override missing from report'
    Assert-Graphics (@(Get-LeonidaGraphicsSwitches $plan -Short) -contains 'JOIN_PARTS=0') `
        'short measurement report is wrong'
    $shown = (Show-LeonidaGraphics $plan 6>&1 | ForEach-Object { "$_" }) -join ' '
    Assert-Graphics ($shown -match 'graphics card: unknown') 'missing adapter is not handled'
    Assert-Graphics ($shown -match 'base, synthetic') 'selected profiles are not displayed'
    Assert-Graphics ($shown -match 'LEONIDA_ASYNC_SUBMIT') 'explicit overrides are not displayed'

    $private = 'C:\Users\Private\captures\frame.png'
    $plan.settings | Add-Member -NotePropertyName LEONIDA_CAPTURE -NotePropertyValue $private
    $reported = (Get-LeonidaGraphicsSwitches $plan) -join ' '
    Assert-Graphics ($reported -match 'LEONIDA_CAPTURE=<set>') 'capture path was not redacted'
    Assert-Graphics ($reported -notmatch 'Private|captures|frame.png') 'report contains a path'
    foreach ($value in @($private, 'private-name', '0123456789abcdef0123456789abcdef', '1,2,3')) {
        Assert-Graphics ((Format-LeonidaGraphicsSwitch 'LEONIDA_SHOT' $value) -eq
            'LEONIDA_SHOT=<set>') 'direct report value was not redacted'
    }
    Assert-Graphics ((Format-LeonidaGraphicsSwitch $private '1') -notmatch 'Private') `
        'an invalid switch name leaked into the report'
    foreach ($value in '0', '1', '3072', '0x10', 'alt', 'on', 'off') {
        Assert-Graphics ((Format-LeonidaGraphicsSwitch 'LEONIDA_QUEUE2' $value) -eq
            "LEONIDA_QUEUE2=$value") 'safe switch value was lost'
    }

    $prior = Save-LeonidaEnvironment
    $env:LEONIDA_ASYNC_SUBMIT = '1'
    $env:LEONIDA_FRAME_LOG = '1'
    Restore-LeonidaEnvironment $prior
    Assert-Graphics ($env:LEONIDA_ASYNC_SUBMIT -eq '0') 'caller override was not restored'
    Assert-Graphics (-not (Test-Path Env:LEONIDA_FRAME_LOG)) `
        'diagnostic switch leaked after restore'

    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        Set-LeonidaEmptyEnvironment 'LEONIDA_EMPTY_TEST'
        # Leave ERROR_ENVVAR_NOT_FOUND behind, which makes Framework's single-variable
        # getter unreliable for empty values. Check the complete environment block instead.
        $null = [Environment]::GetEnvironmentVariable('LEONIDA_NO_SUCH_EMPTY_TEST')
        $prior = Save-LeonidaEnvironment
        Assert-Graphics ($prior.Contains('LEONIDA_EMPTY_TEST')) 'snapshot lost an empty override'
        Assert-Graphics ($prior['LEONIDA_EMPTY_TEST'] -ceq '') 'snapshot changed an empty override'
        Disable-LeonidaGraphics
        $actual = [Environment]::GetEnvironmentVariables()
        Assert-Graphics (-not $actual.Contains('LEONIDA_EMPTY_TEST')) `
            'baseline kept an empty runtime override'
        Restore-LeonidaEnvironment $prior
        $actual = [Environment]::GetEnvironmentVariables()
        Assert-Graphics ($actual.Contains('LEONIDA_EMPTY_TEST')) 'empty caller override was deleted'
        Assert-Graphics ($actual['LEONIDA_EMPTY_TEST'] -ceq '') `
            'empty caller override changed value'
        Set-LeonidaEmptyEnvironment 'LEONIDA_NEW_EMPTY_TEST'
        Restore-LeonidaEnvironment $prior
        $actual = [Environment]::GetEnvironmentVariables()
        Assert-Graphics (-not $actual.Contains('LEONIDA_NEW_EMPTY_TEST')) `
            'new empty override survived restoration'
        [Environment]::SetEnvironmentVariable('LEONIDA_EMPTY_TEST', $null)
    }

    $script:queryMode = 'stderr'
    $plan = Get-LeonidaGraphics 'Invoke-SyntheticLeonida' -WarningAction SilentlyContinue
    Assert-Graphics ($plan.settings.LEONIDA_COPY_PLAIN -eq '1') 'stderr corrupted native JSON'
    $script:queryMode = 'invalid'
    Assert-GraphicsThrows { Get-LeonidaGraphics 'Invoke-SyntheticLeonida' } 'invalid JSON'
    $script:queryMode = 'failure'
    Assert-GraphicsThrows { Get-LeonidaGraphics 'Invoke-SyntheticLeonida' } 'exit 19'
    $script:queryMode = 'valid'

    $controls = 'CONFIG', 'GAME_DIR', 'CACHE_DIR', 'DATA_DIR', 'TEST_TITLES', 'PROFILE',
        'LOG', 'KEEP_WINDOW', 'D3D12_WARP', 'RELAUNCHED_FROM', 'AUDIO_CAPTURE_OF',
        'RENDERDOC', 'RENDERDOC_OUT', 'RENDERDOC_AT', 'RENDERDOC_FUTURE_OPTION'
    foreach ($name in $controls) { Set-Item -LiteralPath "Env:LEONIDA_$name" "preserved-$name" }
    $env:LEONIDA_UNLISTED_RUNTIME_SWITCH = '1'
    $prior = Save-LeonidaEnvironment
    Disable-LeonidaGraphics
    Assert-Graphics ($env:LEONIDA_GRAPHICS_OFF -eq '1') 'baseline did not disable native profiles'
    Assert-Graphics (-not (Test-Path Env:LEONIDA_ASYNC_SUBMIT)) 'baseline kept an override'
    Assert-Graphics (-not (Test-Path Env:LEONIDA_UNLISTED_RUNTIME_SWITCH)) `
        'baseline kept a runtime switch'
    foreach ($name in $controls) {
        Assert-Graphics ((Get-Item -LiteralPath "Env:LEONIDA_$name").Value -eq "preserved-$name") `
            "baseline removed control $name"
    }
    $plan = Get-LeonidaGraphics 'Invoke-SyntheticLeonida'
    Assert-Graphics (@($plan.profiles).Count -eq 0) 'baseline still selected profiles'
    Assert-Graphics (@(Get-LeonidaGraphicsSwitches $plan).Count -eq 0) `
        'baseline report is not empty'

    Restore-LeonidaEnvironment $prior
    Assert-Graphics ($env:LEONIDA_ASYNC_SUBMIT -eq '0') 'baseline lost the caller override'
    Assert-Graphics (-not (Test-Path Env:LEONIDA_GRAPHICS_OFF)) 'baseline leaked into the next run'

    if ($Exe) {
        $Exe = (Resolve-Path -LiteralPath $Exe).Path
        Restore-LeonidaEnvironment @{}
        [IO.Directory]::CreateDirectory($temporary) | Out-Null
        $env:LEONIDA_CONFIG = Join-Path $temporary 'leonida.toml'
        [IO.File]::WriteAllText($env:LEONIDA_CONFIG, '')
        $env:LEONIDA_CACHE_DIR = Join-Path $temporary 'cache'
        $env:LEONIDA_DATA_DIR = Join-Path $temporary 'data'
        $env:LEONIDA_PROFILE = 'none'
        $env:LEONIDA_ASYNC_SUBMIT = '0'
        Push-Location $temporary
        try {
            $plan = Get-LeonidaGraphics $Exe
            Assert-Graphics (@($plan.profiles).Count -eq 0) 'native profile=none was ignored'
            Assert-Graphics ($plan.settings.LEONIDA_ASYNC_SUBMIT -eq '0') 'native override was lost'
            Assert-Graphics ($plan.sources.LEONIDA_ASYNC_SUBMIT -eq 'environment') `
                'wrong native source'
            # Construct non-ASCII text without requiring a BOM in this PowerShell 5.1 script.
            $unicodeName = 'test-' + [char]0x6e2c + [char]0x5b9a
            $catalog = @{ profiles = @(@{
                name = $unicodeName; opt_in = $true; env = @{ LEONIDA_QUEUE2 = '2' }
            }) } | ConvertTo-Json -Depth 4
            [IO.File]::WriteAllText((Join-Path $temporary 'gpu-profiles.local.json'), $catalog)
            $env:LEONIDA_PROFILE = $unicodeName
            $plan = Get-LeonidaGraphics $Exe
            Assert-Graphics ($plan.profiles -contains $unicodeName) `
                'native Unicode profile was corrupted'
            Assert-Graphics ($plan.sources.LEONIDA_QUEUE2 -ceq "profile:$unicodeName") `
                'native Unicode source was corrupted'
            if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
                Set-LeonidaEmptyEnvironment 'LEONIDA_ASYNC_SUBMIT'
                $prior = Save-LeonidaEnvironment
                Disable-LeonidaGraphics
                Restore-LeonidaEnvironment $prior
                $plan = Get-LeonidaGraphics $Exe
                $names = $plan.settings.PSObject.Properties.Name
                Assert-Graphics ('LEONIDA_ASYNC_SUBMIT' -in $names) `
                    'native child did not inherit the restored empty override'
                Assert-Graphics ($plan.settings.LEONIDA_ASYNC_SUBMIT -ceq '') `
                    'native child inherited the wrong empty override value'
                Assert-Graphics ($plan.sources.LEONIDA_ASYNC_SUBMIT -eq 'environment') `
                    'native child did not treat the empty override as explicit'
            }
            Remove-Item Env:LEONIDA_PROFILE
            Disable-LeonidaGraphics
            $plan = Get-LeonidaGraphics $Exe
            Assert-Graphics (@($plan.profiles).Count -eq 0) 'native baseline enabled a profile'
            Assert-Graphics (@(Get-LeonidaGraphicsSwitches $plan).Count -eq 0) `
                'native baseline is not empty'
        } finally { Pop-Location }
        Write-Host 'real native graphics query: passed'
    }
    Write-Host 'graphics launcher checks: passed'
} finally {
    Restore-LeonidaEnvironment $saved
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Recurse -Force }
}
