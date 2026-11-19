@echo off
rem SPDX-License-Identifier: GPL-3.0-or-later
rem run_all.sh --farm's side on the VM (farm_run.sh), started from the PC over ssh as a scheduled task
rem with /IT, so it runs in the logged-in user's session (docs/proof_farm.md); its own errors in run\task.txt
"C:\Program Files\Git\bin\bash.exe" "%~dp0farm_run.sh" > "%~dp0..\..\..\run\task.txt" 2>&1
