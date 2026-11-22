# 段階 2: .NET Framework 2.0〜3.5 用のアプリは、launch の時点で理由つきで断る (対応は 4 以降)。手元に 2.0〜3.5 の
# コンパイラが無いので、net48 の対象の写しのメタデータの版 (v4.0.30319) を v2.0.50727 に書き換えて使う。エンジンは
# 版をファイルから読むだけで、実行はしない (実行すると Windows が 3.5 を入れるよう求めてくる)。
function Test-OldFramework([string]$ArchName, [string]$Exe) {
    $original = Join-Path $root "target\apps\$ArchName\Console48\SmokeConsole48.exe"
    if (-not (Test-Path $original)) { return }
    $copy = Join-Path ([IO.Path]::GetTempPath()) "netdbg-net20-$ArchName\SmokeConsole48.exe"
    New-Item -ItemType Directory -Force (Split-Path $copy) | Out-Null
    $bytes = [IO.File]::ReadAllBytes($original)
    $old = [Text.Encoding]::ASCII.GetBytes('v4.0.30319')
    $new = [Text.Encoding]::ASCII.GetBytes('v2.0.50727')
    # メタデータのルート (BSJB の 16 バイトあと) の版の文字列だけを書き換える。
    for ($i = 16; $i -le $bytes.Length - $old.Length; $i++) {
        if ([Text.Encoding]::ASCII.GetString($bytes, $i - 16, 4) -eq 'BSJB' -and [Text.Encoding]::ASCII.GetString($bytes, $i, $old.Length) -eq 'v4.0.30319') {
            [Array]::Copy($new, 0, $bytes, $i, $new.Length)
            break
        }
    }
    [IO.File]::WriteAllBytes($copy, $bytes)
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'launch' @{ program = $copy }
        Check $ArchName '.NET Framework 2.0〜3.5 のアプリは、launch で理由つきで断る' (-not $r.success -and $r.message -match 'v2\.0\.50727' -and $r.message -match '4 以降') "($($r.message))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '.NET Framework 2.0〜3.5: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}