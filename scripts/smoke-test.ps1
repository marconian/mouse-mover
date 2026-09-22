[CmdletBinding()]
param(
    [string] $Executable = (Join-Path $PSScriptRoot '..\target\release\velune.exe')
)

$ErrorActionPreference = 'Stop'
$Executable = (Resolve-Path $Executable).Path

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class MouseMoverSmoke {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint process);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint message, UIntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern uint RegisterWindowMessage(string name);
    public static IntPtr Find(uint process, out int visible) {
        IntPtr found = IntPtr.Zero;
        int count = 0;
        EnumWindows((hwnd, _) => {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            if (owner != process) return true;
            if (IsWindowVisible(hwnd)) count++;
            var name = new StringBuilder(256);
            GetClassName(hwnd, name, name.Capacity);
            if (name.ToString() == "MouseMover.TrayWindow") found = hwnd;
            return true;
        }, IntPtr.Zero);
        visible = count;
        return found;
    }
}
'@

if (Get-Process -Name 'velune' -ErrorAction SilentlyContinue) {
    throw 'Exit the running Mouse Mover before smoke testing; no existing instance will be stopped.'
}

$app = Start-Process -FilePath $Executable -PassThru
try {
    if (-not $app.WaitForInputIdle(10000)) { throw 'App did not enter its native message loop.' }
    $visible = 0
    $hwnd = [MouseMoverSmoke]::Find($app.Id, [ref] $visible)
    if ($hwnd -eq [IntPtr]::Zero) { throw 'Hidden tray owner window not found.' }
    if ($visible -ne 0) { throw "Expected no visible windows; found $visible." }

    $duplicate = Start-Process -FilePath $Executable -PassThru
    if (-not $duplicate.WaitForExit(10000)) {
        $duplicate.Kill()
        throw 'Duplicate did not exit.'
    }
    if ($duplicate.ExitCode -ne 0) { throw "Duplicate exit code: $($duplicate.ExitCode)" }

    # Exercise notification handling, not an actual lock or Explorer restart.
    [void] [MouseMoverSmoke]::PostMessage($hwnd, 0x02B1, [UIntPtr] 7, [IntPtr]::Zero)
    [void] [MouseMoverSmoke]::PostMessage($hwnd, 0x02B1, [UIntPtr] 8, [IntPtr]::Zero)
    [void] [MouseMoverSmoke]::PostMessage($hwnd, 0x0218, [UIntPtr] 4, [IntPtr]::Zero)
    [void] [MouseMoverSmoke]::PostMessage($hwnd, 0x0218, [UIntPtr] 18, [IntPtr]::Zero)

    $app.Refresh()
    [pscustomobject] @{
        ExecutableBytes = (Get-Item $Executable).Length
        VisibleWindows = $visible
        DuplicateExitCode = $duplicate.ExitCode
        WorkingSetMiB = [math]::Round($app.WorkingSet64 / 1MB, 2)
        PrivateMemoryMiB = [math]::Round($app.PrivateMemorySize64 / 1MB, 2)
        Threads = $app.Threads.Count
        CpuSeconds = $app.TotalProcessorTime.TotalSeconds
    }

    [void] [MouseMoverSmoke]::PostMessage($hwnd, 0x0010, [UIntPtr]::Zero, [IntPtr]::Zero)
    if (-not $app.WaitForExit(10000)) { throw 'App did not close cleanly.' }
    if ($app.ExitCode -ne 0) { throw "App exit code: $($app.ExitCode)" }
    'PASS: hidden window, single instance, notification dispatch, clean exit. No startup changes or deliberate input injection.'
}
finally {
    if (-not $app.HasExited) {
        $app.Kill()
        $app.WaitForExit()
    }
    $app.Dispose()
}