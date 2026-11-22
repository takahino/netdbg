# 段階 1: 同じ行が複数のメソッドにあるときのブレークポイント (netcoredbg と同じ)。ジェネリックのメソッドは、どの型引数の
# 呼び出しでも止まる。ラムダ式のある行は、外側の文で一度だけ止まる (ラムダの本体の中では止まらない)。
function Test-BreakpointLines([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-bplines-$($script:flavor.Dir)-$ArchName.log"
    $lines = Get-Content $source -Encoding UTF8
    $find = { param([string]$Pattern) for ($i = 0; $i -lt $lines.Count; $i++) { if ($lines[$i] -match $Pattern) { return $i + 1 } } }
    $generic = & $find '^\s+Texts\.Add\(text\);'
    $lambda = & $find '^\s+Add\("numbers\.Count\(n => n > a\)"'
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('conformance') })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $generic }, @{ line = $lambda }) })
        [void](Send-DapRequest $s 'configurationDone')
        # ジェネリックの Add<T>: 最初は int、続けて止めていくと double でも止まる。
        $seen = @()
        for ($i = 0; $i -lt 8; $i++) {
            $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
            $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
            $seen += $frame.name
            if ($i -eq 0) {
                # ジェネリックのメソッドの中の型引数 (T) は、その呼び出しでの実際の型 (C# と同じ)。
                $typeName = (Send-DapRequest $s 'evaluate' @{ expression = 'typeof(T).Name'; frameId = $frame.id }).body.result
                $zero = Send-DapRequest $s 'evaluate' @{ expression = 'default(T)'; frameId = $frame.id }
                Check $ArchName 'ジェネリックのメソッドの中の typeof(T) と default(T)' ($typeName -eq '"Int32"' -and "$($zero.body.result):$($zero.body.type)" -eq '0:int') "($typeName, $($zero.body.result):$($zero.body.type) $($zero.message))"
            }
            if ($i -lt 7) { [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId }) }
        }
        Check $ArchName 'ジェネリックのメソッドの行は、どの型引数の呼び出しでも止まる' (($seen -contains 'Conformance.Add<int>()') -and ($seen -contains 'Conformance.Add<double>()')) "($(($seen | Select-Object -Unique) -join ', '))"
        # ジェネリックの行を外して、ラムダ式の行へ。外側の文で 1 回だけ止まる。
        # 止まっている間に外す (動かしてから外すと、外す前に次の当たりが届く)。
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $lambda }) })
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $at = @()
        while ($true) {
            $next = $null
            while (-not $next) {
                # 応答を待つ間に届いた出来事は、$s.Events にたまっている。先にそちらを見る。
                for ($k = 0; $k -lt $s.Events.Count; $k++) { if ($s.Events[$k].event -in 'stopped', 'exited') { $next = $s.Events[$k]; $s.Events.RemoveAt($k); break } }
                if (-not $next) { $m = Read-DapMessage $s 60000; if ($m.type -eq 'event') { [void]$s.Events.Add($m) } }
            }
            if ($next.event -eq 'exited') { break }
            $at += "$(@((Send-DapRequest $s 'stackTrace' @{ threadId = $next.body.threadId; levels = 1 }).body.stackFrames)[0].name)"
            [void](Send-DapRequest $s 'continue' @{ threadId = $next.body.threadId })
        }
        Check $ArchName 'ラムダ式のある行は、外側の文で 1 回だけ止まる' ($at.Count -eq 1 -and $at[0] -eq 'Conformance.Run()') "($($at -join ', '))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '同じ行が複数のメソッドにある: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}