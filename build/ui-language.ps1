param([string]$Executable,[string]$VideoFixture)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path $PSScriptRoot -Parent
if(-not $Executable){$Executable=Join-Path $taskRoot 'target\debug\offline-license-rust.exe'}
. (Join-Path $PSScriptRoot 'ui-common.ps1')
$taskPreference=Join-Path $env:LOCALAPPDATA 'OfflineLicenseBilingualRust\ui-language.txt'
$taskOriginalPreference=if(Test-Path -LiteralPath $taskPreference){[IO.File]::ReadAllBytes($taskPreference)}else{$null}
$taskWorkspace=Join-Path $taskRoot ('work\ui-language-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskWorkspace -Force | Out-Null
$taskChildren=[Collections.Generic.List[Diagnostics.Process]]::new()
function Start-TestApp([string]$taskPath,[string[]]$taskArgs=@()){
    $taskApp=if($taskArgs.Count){Start-Process -FilePath $taskPath -ArgumentList $taskArgs -WindowStyle Hidden -PassThru}else{Start-Process -FilePath $taskPath -WindowStyle Hidden -PassThru}
    $taskChildren.Add($taskApp)
    $taskWindow=Window $taskApp 'RustOfflineWindow' '*'
    if([SmokeWindows]::Text($taskWindow) -notlike '*TEST BUILD*' -and [SmokeWindows]::Text($taskWindow) -notlike '*测试构建*'){throw 'UI acceptance requires watchdog-test-mode; no production viewer is tested.'}
    return @($taskApp,$taskWindow)
}
function Language($taskWindow,[bool]$taskEnglish){
    [SmokeWindows]::PostMessage($taskWindow,0x111,[IntPtr]$(if($taskEnglish){202}else{201}),[IntPtr]::Zero)|Out-Null
    $taskCaption=if($taskEnglish){'Language changed. Content and authorization remain unchanged.'}else{'语言已切换，当前内容和授权状态保持不变。'}
    for($taskAttempt=0;$taskAttempt -lt 50;$taskAttempt++){
        if([SmokeWindows]::Children($taskWindow)|Where-Object{[SmokeWindows]::Text($_) -eq $taskCaption}){return}
        Start-Sleep -Milliseconds 100
    }
    throw 'Language change did not update status'
}
function Close-TestApp($taskApp,$taskWindow){
    [SmokeWindows]::PostMessage($taskWindow,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null
    if(-not $taskApp.WaitForExit(5000)){throw 'Test window failed to close'}
}
function Answer-Password($taskApp,[string]$taskTitle){
    $taskPrompt=Window $taskApp 'RustOfflinePrompt' $taskTitle
    $taskEdit=[SmokeWindows]::Children($taskPrompt)|Where-Object{[SmokeWindows]::Class($_) -eq 'Edit'}|Select-Object -First 1
    [SmokeWindows]::SendMessage($taskEdit,0xC,[IntPtr]::Zero,'test-only-password')|Out-Null
    Click (Button $taskPrompt 'OK')
}
try{
    New-Item -ItemType Directory -Path (Split-Path $taskPreference -Parent) -Force | Out-Null
    [IO.File]::WriteAllText($taskPreference,'zh')
    $taskApp,$taskWindow=Start-TestApp $Executable
    Button $taskWindow '创建发行者密钥'|Out-Null
    Language $taskWindow $true
    Button $taskWindow 'Create issuer key'|Out-Null
    if([SmokeWindows]::Text($taskWindow) -notlike 'Offline Media Packager*'){throw 'English administrator title not updated'}
    $taskDays=[SmokeWindows]::Children($taskWindow)|Where-Object{[SmokeWindows]::GetDlgCtrlID($_) -eq 20}|Select-Object -First 1
    [SmokeWindows]::SendMessage($taskDays,0xC,[IntPtr]::Zero,'17')|Out-Null
    Language $taskWindow $false
    if([SmokeWindows]::Text($taskDays) -ne '17'){throw ('Language switch changed input value; actual=['+[SmokeWindows]::Text($taskDays)+']')}
    Language $taskWindow $true
    Close-TestApp $taskApp $taskWindow
    $taskApp,$taskWindow=Start-TestApp $Executable
    Button $taskWindow 'Create issuer key'|Out-Null
    Write-Output 'ADMIN_BILINGUAL_AND_PERSISTENCE_PASS'
    Click (Button $taskWindow 'Create issuer key')
    $taskKey=Join-Path $taskWorkspace 'test.issuerkey'
    FileDialog $taskApp $taskKey
    Answer-Password $taskApp 'New key password*'
    Answer-Password $taskApp 'Confirm password'
    for($taskAttempt=0;$taskAttempt -lt 100 -and -not(Test-Path -LiteralPath $taskKey);$taskAttempt++){Start-Sleep -Milliseconds 100}
    if(-not(Test-Path -LiteralPath $taskKey)){throw 'Fixture issuer key was not created'}
    Write-Output 'ENGLISH_FILE_DIALOG_AND_PASSWORD_PROMPTS_PASS'
    Click (Button $taskWindow 'Import device & issue license')
    $taskErrorDialog=Window $taskApp '#32770' 'Offline Media License*'
    if(-not([SmokeWindows]::Children($taskErrorDialog)|Where-Object{[SmokeWindows]::Text($_) -eq 'Build or select a content package first'})){throw 'English validation error was not shown'}
    $taskErrorOk=$null
    for($taskAttempt=0;$taskAttempt -lt 50 -and -not $taskErrorOk;$taskAttempt++){
        $taskErrorOk=[SmokeWindows]::Children($taskErrorDialog)|Where-Object{[SmokeWindows]::Class($_) -eq 'Button'}|Select-Object -First 1
        if(-not $taskErrorOk){Start-Sleep -Milliseconds 100}
    }
    if(-not $taskErrorOk){
        [SmokeWindows]::Children($taskErrorDialog)|ForEach-Object{Write-Output ('DIALOG_CONTROL: '+[SmokeWindows]::Class($_)+' / '+[SmokeWindows]::GetDlgCtrlID($_)+' / '+[SmokeWindows]::Text($_))}
        throw 'Validation dialog OK button was not created'
    }
    Click $taskErrorOk
    for($taskAttempt=0;$taskAttempt -lt 50 -and [SmokeWindows]::IsWindowVisible($taskErrorDialog);$taskAttempt++){Start-Sleep -Milliseconds 100}
    if([SmokeWindows]::IsWindowVisible($taskErrorDialog)){throw 'Validation message box did not close'}
    Start-Sleep -Milliseconds 150
    Write-Output 'ENGLISH_VALIDATION_ERROR_PASS'
    $taskText=Join-Path $taskWorkspace '用户文档.txt'
    [IO.File]::WriteAllText($taskText,'客户视频-授权文件.mp4 / Create issuer key')
    Click (Button $taskWindow 'Import & preview')
    FileDialog $taskApp $taskText
    Start-Sleep -Milliseconds 600
    $taskDocument=[SmokeWindows]::Children($taskWindow)|Where-Object{[SmokeWindows]::Class($_) -like '*RICHEDIT*'}|Select-Object -First 1
    if(-not $taskDocument){throw 'Text preview not found'}
    $taskContent=[SmokeWindows]::Text($taskDocument)
    Language $taskWindow $false
    if([SmokeWindows]::Text($taskDocument) -ne $taskContent){throw 'Language switch rewrote user document'}
    Language $taskWindow $true
    Write-Output 'USER_CONTENT_AND_PREVIEW_PRESERVED_PASS'
    if(-not $VideoFixture){$VideoFixture=Join-Path (Split-Path $taskRoot -Parent) 'work\sample.mp4'}
    if(Test-Path -LiteralPath $VideoFixture){
        Click (Button $taskWindow 'Import & preview')
        FileDialog $taskApp ([IO.Path]::GetFullPath($VideoFixture))
        $taskPlaying=$false
        for($taskAttempt=0;$taskAttempt -lt 100;$taskAttempt++){
            if([SmokeWindows]::GetProp($taskWindow,'RustMediaTime').ToInt64() -gt 200){$taskPlaying=$true;break}
            Start-Sleep -Milliseconds 100
        }
        if(-not $taskPlaying){throw 'Encrypted video preview did not begin playback'}
        [SmokeWindows]::PostMessage($taskWindow,0x100,[IntPtr]27,[IntPtr]::Zero)|Out-Null
        if(-not $taskApp.WaitForExit(10000) -or $taskApp.ExitCode -ne 0){throw 'Video preview failed to exit cleanly on Escape'}
        Write-Output 'ENCRYPTED_VIDEO_PREVIEW_ESCAPE_EXIT_PASS'
    }else{
        Write-Output 'VIDEO_ESCAPE_TEST_SKIPPED_NO_FIXTURE'
        Close-TestApp $taskApp $taskWindow
    }
    $taskApp,$taskWindow=Start-TestApp $Executable @('--device')
    Button $taskWindow 'Select EXE & export device report'|Out-Null
    Language $taskWindow $false
    Button $taskWindow '选择内容并导出设备信息'|Out-Null
    Language $taskWindow $true
    Close-TestApp $taskApp $taskWindow
    Write-Output 'DEVICE_BILINGUAL_PASS'
    $taskSetup=Join-Path $taskWorkspace 'Packager-Setup.exe'
    Copy-Item -LiteralPath $Executable -Destination $taskSetup
    $taskApp,$taskWindow=Start-TestApp $taskSetup
    Button $taskWindow 'Install Rust edition'|Out-Null
    Language $taskWindow $false
    Button $taskWindow '安装 Rust 版'|Out-Null
    Language $taskWindow $true
    Close-TestApp $taskApp $taskWindow
    Write-Output 'SETUP_BILINGUAL_PASS'
    $taskContentExe=Join-Path $taskWorkspace 'content.exe'
    $taskStart=[Diagnostics.ProcessStartInfo]::new()
    $taskStart.FileName=$Executable;$taskStart.UseShellExecute=$false;$taskStart.CreateNoWindow=$true
    $taskStart.RedirectStandardInput=$true;$taskStart.RedirectStandardError=$true
    foreach($taskArgument in @('--pack',$taskText,$taskKey,$taskContentExe)){$taskStart.ArgumentList.Add($taskArgument)}
    $taskPack=[Diagnostics.Process]::Start($taskStart)
    $taskPack.StandardInput.WriteLine('test-only-password');$taskPack.StandardInput.Close()
    $taskPackError=$taskPack.StandardError.ReadToEnd();$taskPack.WaitForExit()
    if($taskPack.ExitCode){throw ('Fixture packaging failed: '+$taskPackError)}
    $taskApp,$taskWindow=Start-TestApp $taskContentExe
    Button $taskWindow 'Import license file'|Out-Null
    Language $taskWindow $false
    Button $taskWindow '导入固态链密钥文件'|Out-Null
    Language $taskWindow $true
    Close-TestApp $taskApp $taskWindow
    Write-Output 'RECIPIENT_ACTIVATION_BILINGUAL_PASS'
    Write-Output 'NO_REAL_LICENSE_ACTIVATION_INSTALL_STARTUP_OR_REBOOT_PERFORMED'
}finally{
    foreach($taskProcess in $taskChildren){if(-not $taskProcess.HasExited){$taskProcess.Kill();$taskProcess.WaitForExit()}}
    if($null -ne $taskOriginalPreference){[IO.File]::WriteAllBytes($taskPreference,$taskOriginalPreference)}elseif(Test-Path -LiteralPath $taskPreference){Remove-Item -LiteralPath $taskPreference}
}
