param([string]$Path)
$j = Get-Content $Path -Raw | ConvertFrom-Json
Write-Output ("activeSet=" + $j.activeOperationSet)
foreach ($s in $j.operationSets) {
    $layerNames = ($s.layers | ForEach-Object { $_.name + "(" + $_.id + ")" }) -join ", "
    Write-Output ("SET " + $s.id + " '" + $s.name + "' layers: " + $layerNames)
    foreach ($l in $s.layers) {
        if ($l.name -notmatch 'RT') { continue }
        Write-Output ("--- LAYER '" + $l.name + "' (" + $l.id + ")")
        foreach ($m in $l.buttonMappings) {
            $lp = if ($m.longPress) { $m.longPress.type } else { "none" }
            $st = if ($m.stateConditions) { ($m.stateConditions | ConvertTo-Json -Compress -Depth 6) } else { "" }
            Write-Output ("  btn=" + $m.buttonId + " type=" + $m.action.type + " key=" + $m.action.keyCode + " mouse=" + $m.action.mouseButton + " wheel=" + $m.action.wheelAmount + " subs=[" + (($m.subCommands | ForEach-Object { $_.keyCode }) -join ",") + "] lp=" + $lp + " state=" + $st)
        }
    }
}
Write-Output "=== keyboardRules ==="
foreach ($r in $j.keyboardRules) {
    Write-Output ("rule id=" + $r.id + " enabled=" + $r.enabled + " keyCode=" + $r.keyCode + " ctrl=" + $r.ctrl + " shift=" + $r.shift + " alt=" + $r.alt + " win=" + $r.win + " mapping=" + ($r.mapping | ConvertTo-Json -Compress -Depth 8))
}
Write-Output ("keyboard_rules_enabled=" + $j.globalSettings.keyboardRulesEnabled)
