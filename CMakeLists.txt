cmake_minimum_required(VERSION 3.20)

option(LASTKEY_INCLUDE_VERSION "Include Windows version metadata" OFF)
if(LASTKEY_INCLUDE_VERSION)
  if(NOT DEFINED LASTKEY_VERSION)
    message(FATAL_ERROR "LASTKEY_VERSION is required when LASTKEY_INCLUDE_VERSION is enabled")
  endif()
  if(NOT LASTKEY_VERSION MATCHES "^([0-9]+)\\.([0-9]+)\\.([0-9]+)$")
    message(FATAL_ERROR "LASTKEY_VERSION must use MAJOR.MINOR.PATCH format")
  endif()
  set(LASTKEY_VERSION_MAJOR "${CMAKE_MATCH_1}")
  set(LASTKEY_VERSION_MINOR "${CMAKE_MATCH_2}")
  set(LASTKEY_VERSION_PATCH "${CMAKE_MATCH_3}")
  project(LastKey VERSION "${LASTKEY_VERSION}" LANGUAGES CXX)
else()
  project(LastKey LANGUAGES CXX)
endif()

add_executable(LastKey WIN32 LastKey.cpp resources.rc)
target_compile_features(LastKey PRIVATE cxx_std_17)
set_property(TARGET LastKey PROPERTY MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
target_compile_definitions(LastKey PRIVATE UNICODE _UNICODE)
if(LASTKEY_INCLUDE_VERSION)
  target_compile_definitions(LastKey PRIVATE
    LASTKEY_HAS_VERSION=1
    LASTKEY_VERSION_MAJOR=${LASTKEY_VERSION_MAJOR}
    LASTKEY_VERSION_MINOR=${LASTKEY_VERSION_MINOR}
    LASTKEY_VERSION_PATCH=${LASTKEY_VERSION_PATCH}
    LASTKEY_VERSION_BUILD=0)
endif()
# Visual Studio's Debug configuration uses /RTC1, which is incompatible with /O2.
target_compile_options(LastKey PRIVATE /W4 /permissive- "$<$<CONFIG:Release>:/O2>")
# resources.rc already embeds app.manifest as resource ID 1.
# Disable the linker's automatically generated manifest to avoid a duplicate.
target_link_options(LastKey PRIVATE /INCREMENTAL:NO /MANIFEST:NO)
target_link_libraries(LastKey PRIVATE user32 shell32 ole32)

option(BUILD_TESTING "Build the SOCD state-transition test executable" OFF)
if(BUILD_TESTING)
  enable_testing()
  add_executable(LastKey-state-tests SocdStateTests.cpp)
  target_compile_features(LastKey-state-tests PRIVATE cxx_std_17)
  target_compile_options(LastKey-state-tests PRIVATE /W4 /permissive-)
  add_test(NAME LastKey.state COMMAND LastKey-state-tests)
endif()
