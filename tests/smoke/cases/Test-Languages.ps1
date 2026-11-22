# C# 以外の言語 (VB.NET、F#) で書かれた対象。デバッガーは IL と PDB を見るので、同じように止まり、値を読めること。
# コンパイラが作る名前 (VB のクロージャ $VB$Closure_、状態機械 VB$StateMachine_) の中の変数は、もとの名前で見えること。
# 式は、対象が何語で書かれていても C# の書き方で評価する (docs/DECISIONS.md D-026)。

# 目印ごとに止めて、フレーム名・変数・式を確かめる。$Expect は「目印 = @{ Frame; Locals = @{名前 = 値}; Hidden = @(名前); Evaluate = @{式 = 値} }」。
function Test-LanguageApp([string]$ArchName, [string]$Exe, [string]$Language, [string]$App, [string]$Source, [System.Collections.Specialized.OrderedDictionary]$Expect) {
    if (-not (Test-Path $App)) { return }
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-language-$Language-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $App })
        $byLine = @{}
        foreach ($marker in $Expect.Keys) { $byLine[(Get-MarkerLine $Source $marker)] = $marker }
        $r = Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $Source }; breakpoints = @($byLine.Keys | ForEach-Object { @{ line = $_ } }) }
        Check $ArchName "${Language}: ブレークポイントを置ける" ($r.success -and @($r.body.breakpoints).Count -eq $byLine.Count) "($($r.message))"
        [void](Send-DapRequest $s 'configurationDone')
        $seen = @{}
        # 同じ目印に何度も止まる (ループの中) ので、初めて止まったときだけ確かめる。
        for ($i = 0; $i -lt 20 -and $seen.Count -lt $Expect.Count; $i++) {
            $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
            $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)
            $marker = $byLine[[int]$frames[0].line]
            if ($marker -and -not $seen[$marker]) {
                $seen[$marker] = $true
                $want = $Expect[$marker]
                if ($want.Frame) { Check $ArchName "${Language} @${marker}: フレーム名" ($frames[0].name -eq $want.Frame) "($($frames[0].name))" }
                $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frames[0].id }).body.scopes)[0]
                $locals = @{}
                foreach ($v in @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)) { $locals[$v.name] = $v.value }
                foreach ($name in @($want.Locals.Keys)) { Check $ArchName "${Language} @${marker}: 変数 $name" ($locals[$name] -ceq $want.Locals[$name]) "($($locals[$name]))" }
                foreach ($name in @($want.Hidden)) { if ($name) { Check $ArchName "${Language} @${marker}: コンパイラの都合の変数 $name は見せない" (-not $locals.ContainsKey($name)) } }
                foreach ($expression in @($want.Evaluate.Keys)) {
                    $e = Send-DapRequest $s 'evaluate' @{ expression = $expression; frameId = $frames[0].id }
                    Check $ArchName "${Language} @${marker}: 式 $expression" ($e.success -and $e.body.result -ceq $want.Evaluate[$expression]) "($($e.body.result)$($e.message))"
                }
            }
            [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        }
        Check $ArchName "${Language}: すべての目印で止まる" ($seen.Count -eq $Expect.Count) "($($seen.Keys -join ', '))"
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName "${Language}: 例外なしで終わる" $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}

function Test-Languages([string]$ArchName, [string]$Exe) {
    $vb = [ordered]@{
        'vb-loop'    = @{ Frame = 'Program.Main()'; Locals = @{ n = '10'; sum = '0'; shade = 'Dark'; label = '"vb"' }; Evaluate = @{ 'sum + n' = '10'; 'numbers.Length' = '3'; 'order.Name' = '"A-1"' } }
        'vb-method'  = @{ Frame = 'Order.Total()'; Locals = @{ price = '2.5' }; Evaluate = @{ 'this.Name' = '"A-1"'; 'Count' = '4'; 'price * 2' = '5'; '_tags.Count' = '1' } }
        # ラムダに取り込まれた変数は、クロージャのフィールドになっている。
        'vb-closure' = @{ Frame = 'Program.Scaled()'; Locals = @{ offset = '1'; scale = '3' }; Hidden = @('$VB$Closure_0'); Evaluate = @{ 'offset + scale' = '4'; 'result.Length' = '3' } }
        # await をまたぐ変数は、状態機械のフィールドになっている。
        'vb-async'   = @{ Locals = @{ a = '1'; b = '2'; before = '2' }; Evaluate = @{ 'before + b' = '4' } }
        'vb-end'     = @{ Frame = 'Program.Main()'; Locals = @{ doubled = '120'; awaited = '4' }; Evaluate = @{ 'grown[1]' = '61'; 'label + "!"' = '"vb!"' } }
    }
    Test-LanguageApp $ArchName $Exe 'VB' (Join-Path $root "target\apps\$ArchName\ConsoleVB\SmokeConsoleVB.exe") (Join-Path $root 'tests\apps\ConsoleVB\Program.vb') $vb
    $fs = [ordered]@{
        'fs-call'  = @{ Frame = 'Program.main()'; Locals = @{ total = '6' }; Evaluate = @{ 'person.Name' = '"Ann"'; 'person.Age + 1' = '31'; 'numbers.Length' = '3'; 'numbers.Head' = '1'; 'shape.IsRect' = 'true' } }
        'fs-match' = @{ Frame = 'Program.area()'; Locals = @{ w = '2'; h = '3' }; Evaluate = @{ 'w * h' = '6' } }
        'fs-twice' = @{ Frame = 'Program.twice()'; Locals = @{ x = '6' }; Evaluate = @{ 'x + 1' = '7' } }
        'fs-end'   = @{ Frame = 'Program.main()'; Locals = @{ doubled = '12'; a = '6' }; Evaluate = @{ 'maybe.Value' = '5' } }
    }
    Test-LanguageApp $ArchName $Exe 'F#' (Join-Path $root "target\apps\$ArchName\ConsoleFS\SmokeConsoleFS.exe") (Join-Path $root 'tests\apps\ConsoleFS\Program.fs') $fs
}
