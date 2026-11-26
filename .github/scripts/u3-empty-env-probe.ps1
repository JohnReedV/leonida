param([string]$Settings = (Join-Path (Get-Location) 'scripts\leonida-settings.ps1'))
$ErrorActionPreference = 'Stop'
. $Settings
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class U3NativeEnvironmentProbe {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    static extern IntPtr GetEnvironmentStringsW();
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    static extern bool FreeEnvironmentStringsW(IntPtr block);
    [DllImport("kernel32.dll", ExactSpelling = true)]
    public static extern void SetLastError(uint error);
    public static string Read(string name) {
        IntPtr block = GetEnvironmentStringsW();
        if (block == IntPtr.Zero) throw new Exception("GetEnvironmentStringsW failed");
        try {
            IntPtr at = block;
            while (true) {
                string entry = Marshal.PtrToStringUni(at);
                if (entry.Length == 0) return null;
                if (entry.StartsWith(name + "=", StringComparison.OrdinalIgnoreCase))
                    return entry.Substring(name.Length + 1);
                at = IntPtr.Add(at, (entry.Length + 1) * 2);
            }
        } finally { FreeEnvironmentStringsW(block); }
    }
}
'@
$name = 'LEONIDA_U3_EMPTY_PROBE'
function Show-EmptyState([string]$Stage) {
    $native = [U3NativeEnvironmentProbe]::Read($name)
    $bulk = [Environment]::GetEnvironmentVariables()
    [U3NativeEnvironmentProbe]::SetLastError(203)
    $single = [Environment]::GetEnvironmentVariable($name)
    [U3NativeEnvironmentProbe]::SetLastError(203)
    $testPath = Test-Path "Env:$name"
    [U3NativeEnvironmentProbe]::SetLastError(203)
    $saved = Save-LeonidaEnvironment
    [ordered]@{
        stage = $Stage
        powershell = $PSVersionTable.PSVersion.ToString()
        native_present = ($null -ne $native)
        native_empty = ($null -ne $native -and $native.Length -eq 0)
        bulk_present = $bulk.Contains($name)
        bulk_empty = ($bulk.Contains($name) -and $bulk[$name] -ceq '')
        single_is_null_after_error_203 = ($null -eq $single)
        test_path_after_error_203 = $testPath
        saved_present = $saved.Contains($name)
        saved_empty = ($saved.Contains($name) -and $saved[$name] -ceq '')
    } | ConvertTo-Json -Compress
}
$before = Save-LeonidaEnvironment
try {
    Set-LeonidaEmptyEnvironment $name
    Show-EmptyState 'set-empty'
    $saved = Save-LeonidaEnvironment
    Disable-LeonidaGraphics
    Show-EmptyState 'disabled'
    Restore-LeonidaEnvironment $saved
    Show-EmptyState 'restored'
} finally {
    [Environment]::SetEnvironmentVariable($name, $null)
    Restore-LeonidaEnvironment $before
}
