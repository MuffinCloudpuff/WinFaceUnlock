cmake_minimum_required(VERSION 3.25)

project(winfaceunlock_mediapipe_bridge LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

add_library(winfaceunlock_mediapipe_bridge SHARED
    src/winfaceunlock_mediapipe_bridge.cpp
)

target_include_directories(winfaceunlock_mediapipe_bridge
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_compile_definitions(winfaceunlock_mediapipe_bridge
    PRIVATE
        WINFACEUNLOCK_MEDIAPIPE_BRIDGE_EXPORTS
)

if(NOT DEFINED MEDIAPIPE_INCLUDE_DIR)
    message(FATAL_ERROR "MEDIAPIPE_INCLUDE_DIR must point to the MediaPipe repository/include root")
endif()

if(NOT DEFINED MEDIAPIPE_TASKS_C_LIB)
    message(FATAL_ERROR "MEDIAPIPE_TASKS_C_LIB must point to the MediaPipe Tasks C import/static library")
endif()

target_include_directories(winfaceunlock_mediapipe_bridge
    PRIVATE
        ${MEDIAPIPE_INCLUDE_DIR}
)

target_link_libraries(winfaceunlock_mediapipe_bridge
    PRIVATE
        ${MEDIAPIPE_TASKS_C_LIB}
)

if(MSVC)
    target_compile_options(winfaceunlock_mediapipe_bridge PRIVATE /W4 /permissive-)
endif()

set_target_properties(winfaceunlock_mediapipe_bridge PROPERTIES
    OUTPUT_NAME "winfaceunlock_mediapipe_bridge"
)
