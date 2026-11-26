# SPDX-License-Identifier: MIT
# Files: the Qt Quick app. Build: scripts/app.sh (or cmake -S app -B <dir> -G Ninja).
cmake_minimum_required(VERSION 3.21)
project(asc_files_app VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui DBus Qml Quick QuickControls2 Multimedia Test)
# Previews' code colours (KDE's syntax definitions).
find_package(KF6SyntaxHighlighting REQUIRED)
# GIO, for network locations through gvfs.
find_package(PkgConfig REQUIRED)
pkg_check_modules(GIO REQUIRED IMPORTED_TARGET gio-2.0)
qt_standard_project_setup(REQUIRES 6.8)

# The models and the backend client, shared by the app and its tests.
qt_add_library(asc_files_core STATIC
    src/backend/BackendClient.cpp src/backend/BackendClient.h
    src/backend/Wire.cpp src/backend/Wire.h
    src/models/FolderModel.cpp src/models/FolderModel.h
    src/models/JobModel.cpp src/models/JobModel.h
    src/models/ActionsModel.cpp src/models/ActionsModel.h
    src/models/Browser.cpp src/models/Browser.h
    src/models/ColumnsModel.cpp src/models/ColumnsModel.h
    src/models/Customizations.cpp src/models/Customizations.h
    src/models/FavoritesModel.cpp src/models/FavoritesModel.h
    src/models/FolderViews.cpp src/models/FolderViews.h
    src/models/ItemListModel.cpp src/models/ItemListModel.h
    src/models/RecentModel.cpp src/models/RecentModel.h
    src/models/SearchModel.cpp src/models/SearchModel.h
    src/models/TrashModel.cpp src/models/TrashModel.h
    src/models/PlacesModel.cpp src/models/PlacesModel.h
    src/models/Properties.cpp src/models/Properties.h
    src/models/TabModel.cpp src/models/TabModel.h
    src/models/Transfers.cpp src/models/Transfers.h
    src/ops/Operations.cpp src/ops/Operations.h
    src/render/CodeHighlighter.cpp src/render/CodeHighlighter.h
    src/render/Preview.cpp src/render/Preview.h
    src/render/Sandbox.cpp src/render/Sandbox.h
    src/app/Windows.cpp src/app/Windows.h
    src/desktop/AppIcons.cpp src/desktop/AppIcons.h
    src/desktop/DesktopApps.cpp src/desktop/DesktopApps.h
    src/desktop/DesktopBus.cpp src/desktop/DesktopBus.h
    src/desktop/Disks.cpp src/desktop/Disks.h
    src/desktop/Network.cpp src/desktop/Network.h
    src/desktop/Cloud.cpp src/desktop/Cloud.h
    src/desktop/Integrations.cpp src/desktop/Integrations.h
    src/portal/Chooser.cpp src/portal/Chooser.h
    src/portal/ChooserService.cpp src/portal/ChooserService.h
    src/portal/Placement.cpp src/portal/Placement.h
    src/settings/AppSettings.cpp src/settings/AppSettings.h
    src/theme/SystemTheme.cpp src/theme/SystemTheme.h
    src/thumbnails/Thumbnails.cpp src/thumbnails/Thumbnails.h)
target_include_directories(asc_files_core PUBLIC src)
target_link_libraries(asc_files_core PUBLIC Qt6::Core Qt6::DBus Qt6::Gui Qt6::Quick KF6::SyntaxHighlighting PkgConfig::GIO)
# What About shows: the version, and the commit built (when built from a checkout).
execute_process(COMMAND git rev-parse --short HEAD WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
                OUTPUT_VARIABLE ASC_FILES_COMMIT OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
if(NOT ASC_FILES_COMMIT)
    set(ASC_FILES_COMMIT "unknown")
endif()
target_compile_definitions(asc_files_core PRIVATE ASC_FILES_VERSION="${PROJECT_VERSION}" ASC_FILES_COMMIT="${ASC_FILES_COMMIT}")
target_compile_options(asc_files_core PRIVATE -Wall -Wextra -Werror=return-type)

# The QML module (window, tokens, components), a static plugin the app and the window test link.
qt_add_library(asc_files_qml STATIC)
set_source_files_properties(qml/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)
qt_add_qml_module(asc_files_qml
    URI AscFiles
    VERSION 1.0
    OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/AscFiles
    QML_FILES
        qml/Main.qml
        qml/FolderView.qml
        qml/ItemsView.qml
        qml/OverviewPage.qml
        qml/SearchPage.qml
        qml/Theme.qml
        qml/components/ActionEditor.qml
        qml/components/ActionsMenu.qml
        qml/components/Badge.qml
        qml/components/Breadcrumb.qml
        qml/components/Button.qml
        qml/components/ConfirmDialog.qml
        qml/components/ConflictDialog.qml
        qml/components/ContextMenu.qml
        qml/components/CustomizeDialog.qml
        qml/components/FileRow.qml
        qml/components/FileTile.qml
        qml/components/FolderColumn.qml
        qml/components/Icon.qml
        qml/components/IconButton.qml
        qml/components/ItemRow.qml
        qml/components/JobsIndicator.qml
        qml/components/LeftoversDialog.qml
        qml/components/MediaView.qml
        qml/components/ArchiveView.qml
        qml/components/CompressDialog.qml
        qml/components/PasswordDialog.qml
        qml/components/PrimaryButton.qml
        qml/components/ChooserBar.qml
        qml/components/UnlockDialog.qml
        qml/components/NameDialog.qml
        qml/components/FormatDialog.qml
        qml/components/ServerPasswordDialog.qml
        qml/components/QuestionDialog.qml
        qml/components/NameEditor.qml
        qml/components/OpenWithDialog.qml
        qml/components/OpenWithMenu.qml
        qml/components/PropertiesDialog.qml
        qml/components/Notice.qml
        qml/components/PaneBar.qml
        qml/components/PlaceItem.qml
        qml/components/PreviewContent.qml
        qml/components/RubberBand.qml
        qml/components/SearchField.qml
        qml/components/SectionHeading.qml
        qml/components/Segmented.qml
        qml/components/SettingRow.qml
        qml/components/SettingsDialog.qml
        qml/components/SettingsGroup.qml
        qml/components/SwatchPicker.qml
        qml/components/TabChip.qml
        qml/components/ToggleSwitch.qml
        qml/components/ToolTipText.qml)
target_link_libraries(asc_files_qml PRIVATE Qt6::Quick)

# The helper that decodes images in the sandbox; installed beside the app.
qt_add_executable(asc-files-render src/render/main.cpp)
target_link_libraries(asc-files-render PRIVATE Qt6::Core Qt6::Gui Qt6::Multimedia)

qt_add_executable(asc-files src/main.cpp)
add_dependencies(asc-files asc-files-render)
target_link_libraries(asc-files PRIVATE asc_files_core asc_files_qmlplugin Qt6::Qml Qt6::Quick Qt6::QuickControls2)

# Tests run against the real backend: ASC_FILES_BACKEND names it (scripts/app.sh builds it).
enable_testing()
set(ASC_FILES_BACKEND "" CACHE FILEPATH "The asc-files-backend binary the tests run")
set(ASC_FILES_DOCS "" CACHE FILEPATH "The asc-files-docs document reader the tests run")
set(ASC_FILES_RAR "" CACHE FILEPATH "The asc-files-rar RAR reader the tests run")
foreach(test wire backendclient foldermodel jobmodel browser places itemlists transfers thumbnails operations preview search systemtheme appsettings desktopapps properties actions customizations codehighlighter filemanager1 portal placement disks network cloud)
    qt_add_executable(tst_${test} tests/tst_${test}.cpp)
    target_link_libraries(tst_${test} PRIVATE asc_files_core Qt6::Test)
    # The repository, for fixtures the Rust crates keep.
    target_compile_definitions(tst_${test} PRIVATE ASC_FILES_SOURCE="${CMAKE_CURRENT_SOURCE_DIR}/..")
    add_test(NAME ${test} COMMAND tst_${test})
    set_tests_properties(${test} PROPERTIES ENVIRONMENT "ASC_FILES_BACKEND=${ASC_FILES_BACKEND};ASC_FILES_DOCS=${ASC_FILES_DOCS};ASC_FILES_RAR=${ASC_FILES_RAR};ASC_FILES_RENDER=$<TARGET_FILE:asc-files-render>;QT_QPA_PLATFORM=offscreen")
    add_dependencies(tst_${test} asc-files-render)
endforeach()
# The window loads with the models and logs no QML warning.
# The media player loads a sound (Qt Multimedia, as the app at run time).
qt_add_executable(tst_mediaview tests/tst_mediaview.cpp)
target_link_libraries(tst_mediaview PRIVATE asc_files_core Qt6::Test Qt6::Qml Qt6::Quick Qt6::QuickControls2 asc_files_qmlplugin)
add_test(NAME mediaview COMMAND tst_mediaview)
set_tests_properties(mediaview PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

qt_add_executable(tst_window tests/tst_window.cpp)
target_link_libraries(tst_window PRIVATE asc_files_core Qt6::Test Qt6::Qml Qt6::Quick Qt6::QuickControls2 asc_files_qmlplugin)
add_test(NAME window COMMAND tst_window)
set_tests_properties(window PROPERTIES ENVIRONMENT "ASC_FILES_BACKEND=${ASC_FILES_BACKEND};ASC_FILES_DOCS=${ASC_FILES_DOCS};ASC_FILES_RAR=${ASC_FILES_RAR};ASC_FILES_RENDER=$<TARGET_FILE:asc-files-render>;QT_QPA_PLATFORM=offscreen")
