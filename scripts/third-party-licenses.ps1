# Writes the licence files of every crate built into leonida.exe into one text file, for the
# release zip: the MIT, Apache and Zlib licences of those crates ask for their notices to ship
# with binaries. Run it after a build, so cargo has the crates' sources:
#
#   powershell -ExecutionPolicy Bypass -File scripts\third-party-licenses.ps1 -Out licences.txt

param(
    [Parameter(Mandatory = $true)]
    [string] $Out
)

$ErrorActionPreference = 'Stop'

$manifest = Join-Path $PSScriptRoot '..\Cargo.toml'
$json = cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc `
    --manifest-path $manifest
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
$meta = ($json -join "`n") | ConvertFrom-Json

$packages = @{}
foreach ($p in $meta.packages) { $packages[$p.id] = $p }
$nodes = @{}
foreach ($n in $meta.resolve.nodes) { $nodes[$n.id] = $n }
$members = @{}
foreach ($id in $meta.workspace_members) { $members[$id] = $true }

# The normal dependencies of leonida, followed down: build and dev dependencies are not linked.
$root = @($meta.packages | Where-Object { $_.name -eq 'leonida' -and $members.ContainsKey($_.id) })
if ($root.Count -ne 1) { throw 'the leonida package is not in the workspace' }
$seen = @{}
$queue = New-Object 'System.Collections.Generic.Queue[string]'
$queue.Enqueue($root[0].id)
while ($queue.Count -gt 0) {
    $id = $queue.Dequeue()
    if ($seen.ContainsKey($id)) { continue }
    $seen[$id] = $true
    foreach ($dep in $nodes[$id].deps) {
        if (@($dep.dep_kinds | Where-Object { $null -eq $_.kind }).Count -gt 0) {
            $queue.Enqueue($dep.pkg)
        }
    }
}

$crates = $seen.Keys | Where-Object { -not $members.ContainsKey($_) } |
    ForEach-Object { $packages[$_] } | Sort-Object name, version
$text = New-Object 'System.Collections.Generic.List[string]'
$text.Add('Licences of the third-party crates built into leonida.exe. Leonida Legacy itself is')
$text.Add('under the GPL, version 3 or later: see LICENSE.')
$missing = @()
foreach ($p in $crates) {
    $dir = Split-Path -Parent $p.manifest_path
    $files = @(Get-ChildItem -LiteralPath $dir -File |
        Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE)' })
    $sub = Join-Path $dir 'LICENSES'
    if (Test-Path -LiteralPath $sub -PathType Container) {
        $files += @(Get-ChildItem -LiteralPath $sub -File)
    }
    if ($p.license_file) {
        $named = Get-Item -LiteralPath (Join-Path $dir $p.license_file) `
            -ErrorAction SilentlyContinue
        if ($named -and -not ($files | Where-Object { $_.FullName -eq $named.FullName })) {
            $files += $named
        }
    }
    $text.Add('')
    $text.Add('=' * 78)
    $text.Add("$($p.name) $($p.version): $($p.license)")
    if ($p.repository) { $text.Add($p.repository) }
    if ($files.Count -eq 0) {
        $text.Add('No licence file in the package: the standard text of the licence above applies.')
        $missing += "$($p.name) $($p.version)"
    }
    foreach ($f in ($files | Sort-Object Name)) {
        $text.Add('')
        $text.Add("--- $($f.Name)")
        $text.Add('')
        $text.Add("$(Get-Content -LiteralPath $f.FullName -Raw)".TrimEnd())
    }
}

$path = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out)
[System.IO.File]::WriteAllLines($path, $text)
"$(@($crates).Count) crates written to $path"
if ($missing.Count -gt 0) { "No licence file in: $($missing -join ', ')" }
