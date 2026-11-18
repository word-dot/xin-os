# Ignore ARCH environment variable, always use x86_64 for host compilation
set(ARCH "x86_64")

# Name of the target - use Generic to avoid cross-compilation mode
set(CMAKE_SYSTEM_NAME "Generic")
set(CMAKE_SYSTEM_PROCESSOR ${ARCH})

# Toolchain settings
# Use system gcc with absolute paths
set(CMAKE_C_COMPILER    /usr/bin/gcc)
set(CMAKE_CXX_COMPILER  /usr/bin/g++)
set(AS                  /usr/bin/as)
set(AR                  /usr/bin/ar)
set(OBJCOPY             /usr/bin/objcopy)
set(OBJDUMP             /usr/bin/objdump)
set(SIZE                /usr/bin/size)

set(LD_FLAGS "-nolibc -nostdlib -static --gc-sections -nostartfiles")

set(CMAKE_C_FLAGS   "-std=gnu99 -fdata-sections -ffunction-sections" CACHE INTERNAL "c compiler flags")
set(CMAKE_CXX_FLAGS "-fdata-sections -ffunction-sections" CACHE INTERNAL "cxx compiler flags")
set(CMAKE_ASM_FLAGS "" CACHE INTERNAL "asm compiler flags")

# set(CMAKE_PASS_LIB_FLAGS " -I/home/os/rust/arceos/ulib/axlibc/include ")
# set(CMAKE_C_FLAGS "-nostdinc -fno-builtin -ffreestanding ${CMAKE_PASS_LIB_FLAGS} ${CMAKE_C_FLAGS}")
set(CMAKE_C_FLAGS "-fPIC -fno-builtin -ffreestanding ${CMAKE_C_FLAGS}")
set(CMAKE_CXX_FLAGS "-fPIC -nostdinc -fno-builtin -ffreestanding ${CMAKE_CXX_FLAGS}")

if (APPLE)
    set(CMAKE_EXE_LINKER_FLAGS "-dead_strip" CACHE INTERNAL "exe link flags")
else (APPLE)
    set(CMAKE_EXE_LINKER_FLAGS "-Wl,--gc-sections" CACHE INTERNAL "exe link flags")
endif (APPLE)

SET(CMAKE_C_FLAGS_DEBUG "-O0 -g -ggdb3" CACHE INTERNAL "c debug compiler flags")
SET(CMAKE_CXX_FLAGS_DEBUG "-O0 -g -ggdb3" CACHE INTERNAL "cxx debug compiler flags")
SET(CMAKE_ASM_FLAGS_DEBUG "-g -ggdb3" CACHE INTERNAL "asm debug compiler flags")

SET(CMAKE_C_FLAGS_RELEASE "-O2 -g -ggdb3" CACHE INTERNAL "c release compiler flags")
SET(CMAKE_CXX_FLAGS_RELEASE "-O2 -g -ggdb3" CACHE INTERNAL "cxx release compiler flags")
SET(CMAKE_ASM_FLAGS_RELEASE "" CACHE INTERNAL "asm release compiler flags")
