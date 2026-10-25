; Хуки установщика NSIS (bundle > windows > nsis > installerHooks).
; Файл в UTF-8 с BOM: иначе makensis прочтёт кириллицу в кодовой странице системы.

; Классы файлов из fileAssociations в tauri.windows.conf.json.
!macro SCROBB_FOR_EACH_CLASS MACRO
  !insertmacro ${MACRO} "ScrobbPlayer.M3U"
  !insertmacro ${MACRO} "ScrobbPlayer.PLS"
  !insertmacro ${MACRO} "ScrobbPlayer.XSPF"
  !insertmacro ${MACRO} "ScrobbPlayer.JSPF"
!macroend

; Шаблон Tauri пишет путь к программе без кавычек (а в нём пробел:
; «…\AppData\Local\Scrobb Player\») и подписывает пункт меню по-английски.
!macro SCROBB_FIX_CLASS CLASS
  WriteRegStr SHCTX "Software\Classes\${CLASS}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  ${If} $LANGUAGE == ${LANG_RUSSIAN}
    WriteRegStr SHCTX "Software\Classes\${CLASS}\shell\open" "" "Открыть в ${PRODUCTNAME}"
  ${EndIf}
!macroend

; Проводник перечитывает ассоциации сразу, без перезахода в систему.
!macro SCROBB_NOTIFY_SHELL
  System::Call "shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro SCROBB_FOR_EACH_CLASS SCROBB_FIX_CLASS
  !insertmacro SCROBB_NOTIFY_SHELL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro SCROBB_NOTIFY_SHELL
!macroend
