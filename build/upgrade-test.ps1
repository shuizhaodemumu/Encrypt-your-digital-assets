$ErrorActionPreference='Stop'
$taskRoot=Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'run-rust.ps1')
$taskCase=Get-ChildItem -LiteralPath (Join-Path $taskRoot 'work') -Directory -Filter 'ui-language-*'|
    Where-Object{Test-Path -LiteralPath (Join-Path $_.FullName 'content.exe')}|
    Sort-Object LastWriteTime -Descending|Select-Object -First 1
if(-not $taskCase){throw 'Run bilingual UI acceptance to create a synthetic content fixture first.'}
$taskSource=Join-Path $taskCase.FullName 'content.exe'
$taskOutput=Join-Path $taskCase.FullName ('upgraded-'+[guid]::NewGuid().ToString('N')+'.exe')
$taskSourceHash=(Get-FileHash -LiteralPath $taskSource -Algorithm SHA256).Hash
& (Join-Path $PSScriptRoot 'Upgrade-Content.ps1') -Source $taskSource -Output $taskOutput
function Package-Hash([string]$taskFile){
    $taskStream=[IO.File]::OpenRead($taskFile)
    $taskReader=[IO.BinaryReader]::new($taskStream)
    try{
        $taskStream.Seek(-32,[IO.SeekOrigin]::End)|Out-Null
        if([Text.Encoding]::ASCII.GetString($taskReader.ReadBytes(8)) -ne 'OLEXE001'){throw 'Missing EXE trailer'}
        $taskOffset=$taskReader.ReadUInt64();$taskLength=$taskReader.ReadUInt64()
        if($taskLength -gt 2097152){throw 'Unexpected large synthetic test payload'}
        $taskStream.Position=$taskOffset
        $taskBytes=$taskReader.ReadBytes([int]$taskLength)
        $taskHasher=[Security.Cryptography.SHA256]::Create()
        try{return [Convert]::ToHexString($taskHasher.ComputeHash($taskBytes))}finally{$taskHasher.Dispose()}
    }finally{$taskReader.Dispose();$taskStream.Dispose()}
}
if((Package-Hash $taskSource) -ne (Package-Hash $taskOutput)){throw 'Encrypted payload changed'}
if((Get-FileHash -LiteralPath ($taskSource+'.olmeta')).Hash -ne (Get-FileHash -LiteralPath ($taskOutput+'.olmeta')).Hash){throw 'Administrator record changed'}
$taskTool=Join-Path $taskRoot '程序\ViewerUpdater-OfficeCompat.exe'
$taskKey=Join-Path $taskCase.FullName 'test.issuerkey'
$taskOriginalDigest=Run-Rust -Executable $taskTool -Arguments @('--decrypt-check',$taskSource,$taskKey) -Password 'test-only-password'
$taskUpgradedDigest=Run-Rust -Executable $taskTool -Arguments @('--decrypt-check',$taskOutput,$taskKey) -Password 'test-only-password'
if($taskOriginalDigest -ne $taskUpgradedDigest){throw 'Decrypted fixture differs after upgrade'}
if((Get-FileHash -LiteralPath $taskSource).Hash -ne $taskSourceHash){throw 'Original EXE was modified'}
Write-Output 'UPGRADE_CIPHERTEXT_METADATA_PLAINTEXT_AND_ORIGINAL_PRESERVED_PASS'
