param([Parameter(Mandatory = $true)][string]$Executable)

$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class LimeResourceProbe {
    [DllImport("kernel32.dll", EntryPoint = "LoadLibraryExW", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern IntPtr LoadLibraryEx(string path, IntPtr reserved, uint flags);
    [DllImport("kernel32.dll", EntryPoint = "FindResourceW", ExactSpelling = true, SetLastError = true)]
    public static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll")]
    public static extern bool FreeLibrary(IntPtr module);
}
'@

$path = (Resolve-Path $Executable).Path
# Read resources without executing the binary or resolving its DLL imports.
$module = [LimeResourceProbe]::LoadLibraryEx($path, [IntPtr]::Zero, 0x22)
if ($module -eq [IntPtr]::Zero) {
    throw "Cannot read executable resources: $path"
}
try {
    # RT_GROUP_ICON = 14; RT_MANIFEST = 24. GPUI expects resource ID 1.
    foreach ($resource in @(@{ Type = 14; Name = 'app icon' }, @{ Type = 24; Name = 'manifest' })) {
        $found = [LimeResourceProbe]::FindResource($module, [IntPtr]1, [IntPtr]([int]$resource.Type))
        if ($found -eq [IntPtr]::Zero) {
            throw "Missing $($resource.Name) resource 1 in $path"
        }
        Write-Output "Verified $($resource.Name) resource 1."
    }
}
finally {
    [void][LimeResourceProbe]::FreeLibrary($module)
}
