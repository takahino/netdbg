# クラッシュダンプの試験 (Test-Dump) の道具: プロセスのフル ダンプを書き出す (dbghelp の MiniDumpWriteDump)。
# 64bit の PowerShell で 32bit のプロセスを書き出すと、64bit のダンプ (WOW64) になる。32bit のダンプは、
# SysWOW64 の PowerShell で動かして書き出す。
param([int]$ProcessId, [string]$Path)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class DumpWriter {
    [DllImport("dbghelp.dll", SetLastError = true)]
    static extern bool MiniDumpWriteDump(IntPtr process, uint processId, SafeHandle file, uint type, IntPtr exception, IntPtr user, IntPtr callback);
    public static void Write(System.Diagnostics.Process process, string path) {
        using (var file = new System.IO.FileStream(path, System.IO.FileMode.Create)) {
            // MiniDumpWithFullMemory | WithHandleData | WithUnloadedModules | WithFullMemoryInfo | WithThreadInfo
            if (!MiniDumpWriteDump(process.Handle, (uint)process.Id, file.SafeFileHandle, 0x2 | 0x4 | 0x20 | 0x800 | 0x1000, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        }
    }
}
"@
[DumpWriter]::Write([System.Diagnostics.Process]::GetProcessById($ProcessId), $Path)