$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$diskBus=@{}
try {foreach($disk in Get-CimInstance -Namespace 'root/Microsoft/Windows/Storage' -ClassName 'MSFT_Disk' -ErrorAction Stop){$diskBus[[string]$disk.Number]=[string]$disk.BusType}} catch {}
function Rows([string]$class,[string[]]$fields,[string]$filter='') {
 $parameters=@{ClassName=$class;ErrorAction='Stop'}
 if($filter){$parameters.Filter=$filter}
 $result=@(foreach($row in Get-CimInstance @parameters){
  $record=[ordered]@{}
  foreach($field in $fields){$value=if($class -eq 'Win32_DiskDrive' -and $field -eq 'BusType'){[string]$diskBus[[string]$row.Index]}else{[string]$row.$field};$record[$field]=$value.Trim()}
  [pscustomobject]$record
 })
 return ,$result
}
$hardware=[ordered]@{
 Computer=Rows 'Win32_ComputerSystem' @('Manufacturer','Model')
 BaseBoard=Rows 'Win32_BaseBoard' @('Manufacturer','Product','SerialNumber')
 ComputerProduct=Rows 'Win32_ComputerSystemProduct' @('Name','Vendor','UUID')
 Disks=Rows 'Win32_DiskDrive' @('Model','SerialNumber','InterfaceType','PNPDeviceID','BusType')
 Processors=Rows 'Win32_Processor' @('Name','SerialNumber','ProcessorId')
 Bios=Rows 'Win32_BIOS' @('SerialNumber')
 Memory=Rows 'Win32_PhysicalMemory' @('DeviceLocator','SerialNumber')
 NetworkAdapters=Rows 'Win32_NetworkAdapter' @('Name','MACAddress','NetConnectionStatus') 'PhysicalAdapter=True'
}
$hardware|ConvertTo-Json -Depth 8 -Compress
