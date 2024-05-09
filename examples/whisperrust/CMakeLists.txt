set(TARGET "whisperrust")

include_directories(${CARGO_TARGET_DIR}/cxxbridge/)
include_directories(${CARGO_TARGET_DIR}/cxxbridge/whispercpp/rust/src)
include_directories(${CMAKE_SOURCE_DIR}/rust/whisper_wrapper)
include_directories(${FFMPEG_DIR}/include)


add_executable(${TARGET} whisperrust.cpp
        ${WHISPER_RUST_BRIDGE_CPP}
        ${WHISPER_RUST_LIB_FILE}
        ${WHISPER_RUST_CODE})

include(DefaultTargetOptions)

target_link_libraries(${TARGET} PRIVATE common whisper
        ${CMAKE_THREAD_LIBS_INIT} ${WHISPER_RUST_LIB})
target_link_libraries(${TARGET} PUBLIC avdevice avfilter swscale postproc avutil avcodec avformat swresample)
target_link_directories(${TARGET} PUBLIC ${FFMPEG_DIR}/lib)

add_custom_command(
        OUTPUT ${WHISPER_RUST_BRIDGE_CPP} ${WHISPER_RUST_LIB}
        COMMAND cargo build --manifest-path ${CARGO_MANIFEST} --lib
        DEPENDS ${RUST_CODE_FILE}
        USES_TERMINAL
        COMMENT "Running cargo..."
)
