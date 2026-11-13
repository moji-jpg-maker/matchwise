# whisper-rs-sys forwards CMAKE_* variables, so inject this after project().
# Avoid specializing native code for the CI runner; the x64 baseline retains AVX2.
set(GGML_NATIVE OFF CACHE BOOL "Build without host CPU specialization" FORCE)
set(GGML_AVX512 OFF CACHE BOOL "Disable AVX-512" FORCE)
set(GGML_AVX512_VBMI OFF CACHE BOOL "Disable AVX-512 VBMI" FORCE)
set(GGML_AVX512_VNNI OFF CACHE BOOL "Disable AVX-512 VNNI" FORCE)
set(GGML_AVX512_BF16 OFF CACHE BOOL "Disable AVX-512 BF16" FORCE)
