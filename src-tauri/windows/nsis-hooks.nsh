!macro NSIS_HOOK_POSTINSTALL
  ; Tauri registers the executable command for ssh:// and telnet:// under
  ; Software\Classes. Windows' Default Apps UI also needs application
  ; capabilities and protocol ProgIDs before XTerm is offered as a handler.
  WriteRegStr SHCTX "${MANUPRODUCTKEY}\Capabilities" "ApplicationName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${MANUPRODUCTKEY}\Capabilities" "ApplicationDescription" "Open SSH and Telnet links with ${PRODUCTNAME}"
  WriteRegStr SHCTX "${MANUPRODUCTKEY}\Capabilities" "ApplicationIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "${MANUPRODUCTKEY}\Capabilities\URLAssociations" "ssh" "${PRODUCTNAME}.ssh"
  WriteRegStr SHCTX "${MANUPRODUCTKEY}\Capabilities\URLAssociations" "telnet" "${PRODUCTNAME}.telnet"

  WriteRegStr SHCTX "Software\Classes\${PRODUCTNAME}.ssh" "" "SSH URL"
  WriteRegStr SHCTX "Software\Classes\${PRODUCTNAME}.ssh" "URL Protocol" ""
  WriteRegStr SHCTX "Software\Classes\${PRODUCTNAME}.ssh\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "Software\Classes\${PRODUCTNAME}.ssh\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""

  WriteRegStr SHCTX "Software\Classes\${PRODUCTNAME}.telnet" "" "Telnet URL"
  WriteRegStr SHCTX "Software\Classes\${PRODUCTNAME}.telnet" "URL Protocol" ""
  WriteRegStr SHCTX "Software\Classes\${PRODUCTNAME}.telnet\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "Software\Classes\${PRODUCTNAME}.telnet\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""

  WriteRegStr SHCTX "Software\RegisteredApplications" "${PRODUCTNAME}" "${MANUPRODUCTKEY}\Capabilities"

  ; Tell Explorer and Settings to refresh protocol/default-app associations.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ReadRegStr $R7 SHCTX "Software\RegisteredApplications" "${PRODUCTNAME}"
    ${If} $R7 == "${MANUPRODUCTKEY}\Capabilities"
      DeleteRegValue SHCTX "Software\RegisteredApplications" "${PRODUCTNAME}"
    ${EndIf}

    ReadRegStr $R7 SHCTX "Software\Classes\${PRODUCTNAME}.ssh\shell\open\command" ""
    ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
      DeleteRegKey SHCTX "Software\Classes\${PRODUCTNAME}.ssh"
    ${EndIf}

    ReadRegStr $R7 SHCTX "Software\Classes\${PRODUCTNAME}.telnet\shell\open\command" ""
    ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
      DeleteRegKey SHCTX "Software\Classes\${PRODUCTNAME}.telnet"
    ${EndIf}

    DeleteRegKey SHCTX "${MANUPRODUCTKEY}\Capabilities"

    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$INSTDIR\data"
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
