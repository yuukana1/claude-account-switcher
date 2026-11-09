Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class WinProbe {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();

    public static List<string> List(uint target) {
        var result = new List<string>();
        EnumWindows((h, l) => {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid == target) {
                var cls = new StringBuilder(256);
                GetClassNameW(h, cls, 256);
                var title = new StringBuilder(256);
                GetWindowTextW(h, title, 256);
                result.Add(string.Format("hwnd={0} visible={1} class={2} title={3}",
                    h, IsWindowVisible(h), cls, title));
            }
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
"@
[void][WinProbe]::SetProcessDPIAware()
$proc = Get-Process | Where-Object { $_.ProcessName -like "*Claude*Account*" } | Select-Object -First 1
if (-not $proc) { Write-Output "not running"; exit 0 }
[WinProbe]::List([uint32]$proc.Id) | Write-Output
