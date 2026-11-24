; Purrzip by GmCat. Extra installer steps (Tauri runs these macros inside its installer).
; Gives .purr files their own icon: the installer registers the file type as "Purrzip archive"
; (the name in tauri.conf.json) with the app's icon; this points it at purr.ico instead.
!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\Purrzip archive\DefaultIcon" "" "$\"$INSTDIR\purr.ico$\",0"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
