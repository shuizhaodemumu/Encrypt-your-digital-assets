$ErrorActionPreference='Stop'
$scripts=Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1' -Recurse
foreach($file in $scripts){
 $tokens=$null;$errors=$null
 [void][Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors)
 if($errors.Count){throw ("PowerShell parse errors in "+$file.Name+": "+($errors.Message -join '; '))}
}
Write-Output ("PUBLIC_POWERSHELL_SYNTAX_PASS: "+$scripts.Count+" scripts")
