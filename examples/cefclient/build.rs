#[cfg(target_os = "windows")]
include!("src/shared/resources.rs");

#[cfg(target_os = "windows")]
fn main() {
    winres::WindowsResource::new()
        .set_icon_with_id("resources/win/cefclient.ico", &IDR_MAINFRAME.to_string())
        .set_icon_with_id("resources/win/small.ico", &IDI_SMALL.to_string())
        .append_rc_content(&format!(
            r#"
                #include "windows.h"

                {IDS_BINARY_TRANSFER_HTML} {BINARY} "resources\\binary_transfer.html"
                {IDS_BINDING_HTML} {BINARY} "resources\\binding.html"
                {IDS_CONFIG_HTML} {BINARY} "resources\\config.html"
                {IDS_DIALOGS_HTML} {BINARY} "resources\\dialogs.html"
                {IDS_DRAGGABLE_HTML} {BINARY} "resources\\draggable.html"
                {IDS_HANG_HTML} {BINARY} "resources\\hang.html"
                {IDS_IPC_PERFORMANCE_HTML} {BINARY} "resources\\ipc_performance.html"
                {IDS_LOCALSTORAGE_HTML} {BINARY} "resources\\localstorage.html"
                {IDS_LOGO_PNG} {BINARY} "resources\\logo.png"
                {IDS_MEDIA_ROUTER_HTML} {BINARY} "resources\\media_router.html"
                {IDS_MENU_ICON_1X_PNG} {BINARY} "resources\\menu_icon.1x.png"
                {IDS_MENU_ICON_2X_PNG} {BINARY} "resources\\menu_icon.2x.png"
                {IDS_OSRTEST_HTML} {BINARY} "..\\tests_shared\\resources\\osr_test.html"
                {IDS_OTHER_TESTS_HTML} {BINARY} "resources\\other_tests.html"
                {IDS_PDF_HTML} {BINARY} "..\\tests_shared\\resources\\pdf.html"
                {IDS_PDF_PDF} {BINARY} "..\\tests_shared\\resources\\pdf.pdf"
                {IDS_PERFORMANCE_HTML} {BINARY} "resources\\performance.html"
                {IDS_PERFORMANCE2_HTML} {BINARY} "resources\\performance2.html"
                {IDS_PIP_HTML} {BINARY} "resources\\pip.html"
                {IDS_PREFERENCES_HTML} {BINARY} "resources\\preferences.html"
                {IDS_RESPONSE_FILTER_HTML} {BINARY} "resources\\response_filter.html"
                {IDS_SERVER_HTML} {BINARY} "resources\\server.html"
                {IDS_TASK_MANAGER_HTML} {BINARY} "resources\\task_manager.html"
                {IDS_TRANSPARENCY_HTML} {BINARY} "resources\\transparency.html"
                {IDS_URLREQUEST_HTML} {BINARY} "resources\\urlrequest.html"
                {IDS_WEBSOCKET_HTML} {BINARY} "resources\\websocket.html"
                {IDS_WINDOW_HTML} {BINARY} "resources\\window.html"
                {IDS_WINDOW_ICON_1X_PNG} {BINARY} "..\\tests_shared\\resources\\window_icon.1x.png"
                {IDS_WINDOW_ICON_2X_PNG} {BINARY} "..\\tests_shared\\resources\\window_icon.2x.png"
                {IDS_XMLHTTPREQUEST_HTML} {BINARY} "resources\\xmlhttprequest.html"

                {IDR_MAINFRAME} MENU
                BEGIN
                    POPUP "&File"
                    BEGIN
                        MENUITEM "&Find...",                    {ID_FIND}
                        MENUITEM SEPARATOR
                        MENUITEM "E&xit",                       {IDM_EXIT}
                    END
                    POPUP "&Help"
                    BEGIN
                        MENUITEM "&About ...",                  {IDM_ABOUT}
                    END
                    POPUP "Tests"
                    BEGIN
                        MENUITEM "Get Source",                  {ID_TESTS_GETSOURCE}
                        MENUITEM "Get Text",                    {ID_TESTS_GETTEXT}
                        MENUITEM "New Window",                  {ID_TESTS_WINDOW_NEW}
                        MENUITEM "Popup Window",                {ID_TESTS_WINDOW_POPUP}
                        MENUITEM "Request",                     {ID_TESTS_REQUEST}
                        MENUITEM "Zoom In",                     {ID_TESTS_ZOOM_IN}
                        MENUITEM "Zoom Out",                    {ID_TESTS_ZOOM_OUT}
                        MENUITEM "Zoom Reset",                  {ID_TESTS_ZOOM_RESET}
                        MENUITEM "Set FPS",                     {ID_TESTS_OSR_FPS}
                        MENUITEM "Set Scale Factor",            {ID_TESTS_OSR_DSF}
                        MENUITEM "Begin Tracing",               {ID_TESTS_TRACING_BEGIN}
                        MENUITEM "End Tracing",                 {ID_TESTS_TRACING_END}
                        MENUITEM "Print",                       {ID_TESTS_PRINT}
                        MENUITEM "Print to PDF",                {ID_TESTS_PRINT_TO_PDF}
                        MENUITEM "Mute Audio",                  {ID_TESTS_MUTE_AUDIO}
                        MENUITEM "Unmute Audio",                {ID_TESTS_UNMUTE_AUDIO}
                        MENUITEM "Other Tests",                 {ID_TESTS_OTHER_TESTS}
                    END
                END

                {IDR_MAINFRAME} ACCELERATORS
                BEGIN
                    "?",            {IDM_ABOUT},              ASCII,  ALT
                    "/",            {IDM_ABOUT},              ASCII,  ALT
                END

                {IDD_ABOUTBOX} DIALOG  22, 17, 230, 75
                STYLE DS_SETFONT | DS_MODALFRAME | WS_CAPTION | WS_SYSMENU
                CAPTION "About"
                FONT 8, "System"
                BEGIN
                    ICON            {IDR_MAINFRAME},{IDC_MYICON},14,9,16,16
                    LTEXT           "cefclient Version 1.0",{IDC_STATIC},49,10,119,8,SS_NOPREFIX
                    LTEXT           "Copyright (C) 2008",{IDC_STATIC},49,20,119,8
                    DEFPUSHBUTTON   "OK",IDOK,195,6,30,11,WS_GROUP
                END

                STRINGTABLE
                BEGIN
                    {IDS_APP_TITLE}           "cefclient"
                    {IDR_MAINFRAME}           "CEFCLIENT"
                END
            "#
        ))
        .compile()
        .unwrap();
}

#[cfg(not(target_os = "windows"))]
fn main() {}
