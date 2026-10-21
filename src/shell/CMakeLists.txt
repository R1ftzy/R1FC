cmake_minimum_required(VERSION 3.20)
project(R1fcShell LANGUAGES CXX)

add_library(r1fc-shell SHARED
    shell.cpp
    shell.def
)

target_compile_features(r1fc-shell PRIVATE cxx_std_17)
target_compile_definitions(r1fc-shell PRIVATE
    UNICODE
    _UNICODE
    WIN32_LEAN_AND_MEAN
    NOMINMAX
)
target_link_libraries(r1fc-shell PRIVATE
    ole32
    shell32
    shlwapi
    user32
)

set_target_properties(r1fc-shell PROPERTIES
    OUTPUT_NAME "r1fc-shell"
    PREFIX ""
)

option(R1FC_BUILD_HARNESS "Build the shell extension test harness" ON)

if (R1FC_BUILD_HARNESS)
    add_executable(r1fc-shell-harness
        ${CMAKE_CURRENT_SOURCE_DIR}/../../tests/shell_harness.cpp
    )
    target_compile_features(r1fc-shell-harness PRIVATE cxx_std_17)
    target_compile_definitions(r1fc-shell-harness PRIVATE
        UNICODE
        _UNICODE
        WIN32_LEAN_AND_MEAN
        NOMINMAX
    )
    target_link_libraries(r1fc-shell-harness PRIVATE
        ole32
        shell32
        shlwapi
        user32
    )
endif()
