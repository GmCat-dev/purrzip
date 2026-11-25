; Purrzip by GmCat. Extra installer steps (Tauri runs these macros inside its installer).
;
; 1. .purr files get their own icon: the installer registers the file type as "Purrzip archive" (the name in
;    tauri.conf.json) with the app's icon; this points it at purr.ico instead.
; 2. A "Purrzip" right-click menu in File Explorer:
;      archives:            Open with Purrzip, Extract here, Extract to a new folder, Extract files..., Test archive,
;                           Add to archive..., Add to a .purr archive, Add to a .zip archive
;      other files/folders: Add to archive..., Add to a .purr archive, Add to a .zip archive
;    Each entry starts Purrzip with a mode (--extract-here, ...) and the file; picking several files starts it once
;    per file and the open window gathers them. On Windows 11 these sit under "Show more options".

!define PZ_CLASSES "Software\Classes"
!define PZ_NOT_ARCHIVES "NOT (System.FileExtension:=.zip OR System.FileExtension:=.7z OR System.FileExtension:=.purr OR System.FileExtension:=.rar OR System.FileExtension:=.tar OR System.FileExtension:=.tgz OR System.FileExtension:=.gz OR System.FileExtension:=.bz2 OR System.FileExtension:=.xz OR System.FileExtension:=.zst OR System.FileExtension:=.tbz2 OR System.FileExtension:=.txz OR System.FileExtension:=.iso OR System.FileExtension:=.cab)"

; one menu entry: KEY = parent "shell" key, NAME = sort key, TEXT, ARGS, FLAGS (0x20 = line above)
!macro PZ_ITEM KEY NAME TEXT ARGS FLAGS
  WriteRegStr SHCTX "${KEY}\${NAME}" "MUIVerb" "${TEXT}"
  WriteRegDWORD SHCTX "${KEY}\${NAME}" "CommandFlags" ${FLAGS}
  WriteRegStr SHCTX "${KEY}\${NAME}\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" ${ARGS} "%1"'
!macroend

!macro PZ_MENU KEY
  WriteRegStr SHCTX "${KEY}" "MUIVerb" "Purrzip"
  WriteRegStr SHCTX "${KEY}" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "${KEY}" "SubCommands" ""
!macroend

!macro PZ_ADD_ITEMS KEY FIRSTFLAGS
  !insertmacro PZ_ITEM "${KEY}\shell" "6add" "Add to archive..." "--add" ${FIRSTFLAGS}
  !insertmacro PZ_ITEM "${KEY}\shell" "7addpurr" "Add to a .purr archive" "--add-purr" 0
  !insertmacro PZ_ITEM "${KEY}\shell" "8addzip" "Add to a .zip archive" "--add-zip" 0
!macroend

!macro PZ_ARCHIVE_MENU EXT
  !insertmacro PZ_MENU "${PZ_CLASSES}\SystemFileAssociations\.${EXT}\shell\Purrzip"
  !insertmacro PZ_ITEM "${PZ_CLASSES}\SystemFileAssociations\.${EXT}\shell\Purrzip\shell" "1open" "Open with Purrzip" "" 0
  !insertmacro PZ_ITEM "${PZ_CLASSES}\SystemFileAssociations\.${EXT}\shell\Purrzip\shell" "2here" "Extract here" "--extract-here" 0
  !insertmacro PZ_ITEM "${PZ_CLASSES}\SystemFileAssociations\.${EXT}\shell\Purrzip\shell" "3new" "Extract to a new folder" "--extract-new" 0
  !insertmacro PZ_ITEM "${PZ_CLASSES}\SystemFileAssociations\.${EXT}\shell\Purrzip\shell" "4ask" "Extract files..." "--extract-ask" 0
  !insertmacro PZ_ITEM "${PZ_CLASSES}\SystemFileAssociations\.${EXT}\shell\Purrzip\shell" "5test" "Test archive" "--test" 0x20
  !insertmacro PZ_ADD_ITEMS "${PZ_CLASSES}\SystemFileAssociations\.${EXT}\shell\Purrzip" 0x20
!macroend

!macro PZ_UNMENU EXT
  DeleteRegKey SHCTX "${PZ_CLASSES}\SystemFileAssociations\.${EXT}\shell\Purrzip"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "${PZ_CLASSES}\Purrzip archive\DefaultIcon" "" "$\"$INSTDIR\purr.ico$\",0"

  !insertmacro PZ_ARCHIVE_MENU zip
  !insertmacro PZ_ARCHIVE_MENU 7z
  !insertmacro PZ_ARCHIVE_MENU purr
  !insertmacro PZ_ARCHIVE_MENU rar
  !insertmacro PZ_ARCHIVE_MENU tar
  !insertmacro PZ_ARCHIVE_MENU tgz
  !insertmacro PZ_ARCHIVE_MENU gz
  !insertmacro PZ_ARCHIVE_MENU bz2
  !insertmacro PZ_ARCHIVE_MENU xz
  !insertmacro PZ_ARCHIVE_MENU zst
  !insertmacro PZ_ARCHIVE_MENU tbz2
  !insertmacro PZ_ARCHIVE_MENU txz
  !insertmacro PZ_ARCHIVE_MENU iso
  !insertmacro PZ_ARCHIVE_MENU cab

  ; every other file (archives have their own menu above) and every folder
  !insertmacro PZ_MENU "${PZ_CLASSES}\*\shell\Purrzip"
  WriteRegStr SHCTX "${PZ_CLASSES}\*\shell\Purrzip" "AppliesTo" "${PZ_NOT_ARCHIVES}"
  !insertmacro PZ_ADD_ITEMS "${PZ_CLASSES}\*\shell\Purrzip" 0
  !insertmacro PZ_MENU "${PZ_CLASSES}\Directory\shell\Purrzip"
  !insertmacro PZ_ADD_ITEMS "${PZ_CLASSES}\Directory\shell\Purrzip" 0

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey SHCTX "${PZ_CLASSES}\*\shell\Purrzip"
  DeleteRegKey SHCTX "${PZ_CLASSES}\Directory\shell\Purrzip"
  !insertmacro PZ_UNMENU zip
  !insertmacro PZ_UNMENU 7z
  !insertmacro PZ_UNMENU purr
  !insertmacro PZ_UNMENU rar
  !insertmacro PZ_UNMENU tar
  !insertmacro PZ_UNMENU tgz
  !insertmacro PZ_UNMENU gz
  !insertmacro PZ_UNMENU bz2
  !insertmacro PZ_UNMENU xz
  !insertmacro PZ_UNMENU zst
  !insertmacro PZ_UNMENU tbz2
  !insertmacro PZ_UNMENU txz
  !insertmacro PZ_UNMENU iso
  !insertmacro PZ_UNMENU cab
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
