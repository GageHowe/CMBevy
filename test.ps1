$job1 = Start-Process "notepad.exe" -PassThru
$job2 = Start-Process "calc.exe" -PassThru

try {
    Read-Host "Press Enter to exit"
}
finally {
    Write-Host "Killing PID $($job1.Id) and $($job2.Id)"
    Stop-Process -Id $job1.Id -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $job2.Id -Force -ErrorAction SilentlyContinue
}