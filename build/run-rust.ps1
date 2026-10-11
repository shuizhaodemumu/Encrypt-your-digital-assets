function Run-Rust {
 param([string]$Executable,[string[]]$Arguments,[string]$Password='')
 $start=[Diagnostics.ProcessStartInfo]::new()
 $start.FileName=$Executable
 $start.UseShellExecute=$false
 $start.CreateNoWindow=$true
 $start.RedirectStandardInput=$true
 $start.RedirectStandardOutput=$true
 $start.RedirectStandardError=$true
 foreach($argument in $Arguments){$start.ArgumentList.Add($argument)}
 $process=[Diagnostics.Process]::Start($start)
 $process.StandardInput.WriteLine($Password)
 $process.StandardInput.Close()
 $output=$process.StandardOutput.ReadToEnd()
 $errors=$process.StandardError.ReadToEnd()
 $process.WaitForExit()
 if($process.ExitCode){throw "Rust exited $($process.ExitCode): $errors"}
 if($output){Write-Output $output.TrimEnd()}
}
