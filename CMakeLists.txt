cmake_minimum_required(VERSION 3.16)
project(xiaohong_perception LANGUAGES C CXX)

if(NOT DEFINED OHOS_ARCH OR NOT DEFINED RUST_TARGET OR NOT DEFINED OHOS_NDK_HOME)
  message(FATAL_ERROR "Set OHOS_ARCH, RUST_TARGET and OHOS_NDK_HOME")
endif()

if(OHOS_ARCH STREQUAL "arm64-v8a")
  set(OHOS_ABI "aarch64-linux-ohos")
elseif(OHOS_ARCH STREQUAL "x86_64")
  set(OHOS_ABI "x86_64-linux-ohos")
else()
  message(FATAL_ERROR "Supported OHOS_ARCH values: arm64-v8a, x86_64")
endif()
if(NOT RUST_TARGET STREQUAL OHOS_ABI)
  message(FATAL_ERROR "RUST_TARGET must match ${OHOS_ARCH}: ${OHOS_ABI}")
endif()

set(OHOS_TOOLCHAIN "${OHOS_NDK_HOME}/native/build/cmake/ohos.toolchain.cmake")
if(NOT EXISTS "${OHOS_TOOLCHAIN}")
  message(FATAL_ERROR "OHOS NDK CMake toolchain not found: ${OHOS_TOOLCHAIN}")
endif()

set(RUST_LIB "${CMAKE_CURRENT_SOURCE_DIR}/target/${RUST_TARGET}/release/libxiaohong_perception.a")
add_custom_command(
  OUTPUT "${RUST_LIB}"
  COMMAND cargo build --locked --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml" --release --target "${RUST_TARGET}"
  WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
  DEPENDS Cargo.toml Cargo.lock src/lib.rs src/pipeline.rs src/types.rs
  VERBATIM)
add_custom_target(xiaohong_perception_rust DEPENDS "${RUST_LIB}")

add_library(xiaohong_perception SHARED napi/napi_init.cpp)
add_dependencies(xiaohong_perception xiaohong_perception_rust)
target_include_directories(xiaohong_perception PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_link_libraries(xiaohong_perception PRIVATE "${RUST_LIB}" ace_napi.z hilog_ndk.z c++_shared)
target_compile_features(xiaohong_perception PRIVATE cxx_std_17)
target_compile_options(xiaohong_perception PRIVATE -fvisibility=hidden)
target_link_options(xiaohong_perception PRIVATE -Wl,--no-undefined)
set_target_properties(xiaohong_perception PROPERTIES OUTPUT_NAME xiaohong_perception)

include(CTest)
if(BUILD_TESTING)
  add_executable(ffi_smoke tests/ffi_smoke.c)
  target_include_directories(ffi_smoke PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/include")
  target_link_libraries(ffi_smoke PRIVATE xiaohong_perception)
  add_dependencies(ffi_smoke xiaohong_perception)
endif()