cmake_minimum_required(VERSION 3.5)

project(demo-firmware LANGUAGES C ASM)

include(FetchContent)

set(
    STREAM_PORT
    "RTT" CACHE STRING
    "Trace-recorder stream port to use")
set_property(CACHE STREAM_PORT PROPERTY STRINGS RTT TCP)
message(STATUS "Stream port: ${STREAM_PORT}")

if("${STREAM_PORT}" STREQUAL "RTT")
    set(STREAM_PORT_RTT TRUE)
    set(RENODE_SCRIPT "renode/emulate_trc_rtt.resc")
else()
    set(STREAM_PORT_TCP TRUE)
    set(RENODE_SCRIPT "renode/emulate_trc_tcp.resc")
endif()

message(STATUS "Fetching dependencies")

FetchContent_Declare(
    freertos
    GIT_REPOSITORY "https://github.com/FreeRTOS/FreeRTOS-Kernel.git"
    GIT_TAG "V10.5.0"
)

FetchContent_Declare(
    freertostcp
    GIT_REPOSITORY "https://github.com/auxoncorp/FreeRTOS-Plus-TCP.git"
    GIT_TAG "V3.1.0-1-g2359a20"
)

FetchContent_Declare(
    tracerecorder
    GIT_REPOSITORY "https://github.com/percepio/TraceRecorderSource.git"
    GIT_TAG "Tz4/4.8/v4.8.2"
)

FetchContent_Declare(
    stm32cubef4
    GIT_REPOSITORY "https://github.com/STMicroelectronics/STM32CubeF4.git"
    GIT_TAG "v1.27.1"
)

# Use FetchContent_Populate instead of FetchContent_MakeAvailable because
# we're defining build rules locally and don't want cmake to automatically
# call add_subdirectory on these
FetchContent_Populate(freertos)
FetchContent_Populate(freertostcp)
FetchContent_Populate(tracerecorder)
FetchContent_Populate(stm32cubef4)

set(
    KERNEL_SOURCES
    ${freertos_SOURCE_DIR}/portable/GCC/ARM_CM4F/port.c
    ${freertos_SOURCE_DIR}/portable/MemMang/heap_4.c
    ${freertos_SOURCE_DIR}/queue.c
    ${freertos_SOURCE_DIR}/tasks.c
    ${freertos_SOURCE_DIR}/timers.c
    ${freertos_SOURCE_DIR}/event_groups.c
    ${freertos_SOURCE_DIR}/stream_buffer.c
    ${freertos_SOURCE_DIR}/list.c)
set(
    KERNEL_INCLUDES
    ${freertos_SOURCE_DIR}/include
    ${freertos_SOURCE_DIR}/portable/GCC/ARM_CM3)

set(
    TCPIP_SOURCES
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_DNS.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_DHCP.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_ARP.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_ICMP.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_Stream_Buffer.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_IP.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_TCP_IP.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_TCP_Transmission.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_TCP_WIN.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_TCP_State_Handling.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_TCP_Reception.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_TCP_Utils.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_UDP_IP.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_Sockets.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_IP_Utils.c
    ${freertostcp_SOURCE_DIR}/source/FreeRTOS_IP_Timers.c
    ${freertostcp_SOURCE_DIR}/source/portable/BufferManagement/BufferAllocation_2.c
    ${freertostcp_SOURCE_DIR}/source/portable/NetworkInterface/Common/phyHandling.c
    ${freertostcp_SOURCE_DIR}/source/portable/NetworkInterface/STM32Fxx/NetworkInterface.c
    ${freertostcp_SOURCE_DIR}/source/portable/NetworkInterface/STM32Fxx/stm32fxx_hal_eth.c)
set(
    TCPIP_INCLUDES
    ${freertostcp_SOURCE_DIR}/source/include
    ${freertostcp_SOURCE_DIR}/source/portable/NetworkInterface/include
    ${freertostcp_SOURCE_DIR}/source/portable/NetworkInterface/STM32Fxx
    ${freertostcp_SOURCE_DIR}/source/portable/Compiler/GCC)

set(
    TRACING_SOURCES
    ${tracerecorder_SOURCE_DIR}/kernelports/FreeRTOS/trcKernelPort.c
    ${tracerecorder_SOURCE_DIR}/trcAssert.c
    ${tracerecorder_SOURCE_DIR}/trcCounter.c
    ${tracerecorder_SOURCE_DIR}/trcDiagnostics.c
    ${tracerecorder_SOURCE_DIR}/trcEntryTable.c
    ${tracerecorder_SOURCE_DIR}/trcError.c
    ${tracerecorder_SOURCE_DIR}/trcEventBuffer.c
    ${tracerecorder_SOURCE_DIR}/trcEvent.c
    ${tracerecorder_SOURCE_DIR}/trcExtension.c
    ${tracerecorder_SOURCE_DIR}/trcHardwarePort.c
    ${tracerecorder_SOURCE_DIR}/trcHeap.c
    ${tracerecorder_SOURCE_DIR}/trcInternalEventBuffer.c
    ${tracerecorder_SOURCE_DIR}/trcInterval.c
    ${tracerecorder_SOURCE_DIR}/trcISR.c
    ${tracerecorder_SOURCE_DIR}/trcMultiCoreEventBuffer.c
    ${tracerecorder_SOURCE_DIR}/trcObject.c
    ${tracerecorder_SOURCE_DIR}/trcPrint.c
    ${tracerecorder_SOURCE_DIR}/trcStackMonitor.c
    ${tracerecorder_SOURCE_DIR}/trcStateMachine.c
    ${tracerecorder_SOURCE_DIR}/trcStaticBuffer.c
    ${tracerecorder_SOURCE_DIR}/trcString.c
    ${tracerecorder_SOURCE_DIR}/trcTask.c
    ${tracerecorder_SOURCE_DIR}/trcTimestamp.c
    ${tracerecorder_SOURCE_DIR}/trcStreamingRecorder.c)
set(
    TRACING_INCLUDES
    ${tracerecorder_SOURCE_DIR}/kernelports/FreeRTOS/include
    ${tracerecorder_SOURCE_DIR}/include)

if(DEFINED STREAM_PORT_RTT)
    list(
        APPEND
        TRACING_SOURCES
        ${tracerecorder_SOURCE_DIR}/streamports/Jlink_RTT/SEGGER_RTT.c
        ${tracerecorder_SOURCE_DIR}/streamports/Jlink_RTT/trcStreamPort.c)
    list(
        APPEND
        TRACING_INCLUDES
        ${tracerecorder_SOURCE_DIR}/streamports/Jlink_RTT/include)
endif()

set(
    HAL_SOURCES
    ${stm32cubef4_SOURCE_DIR}/Drivers/STM32F4xx_HAL_Driver/Src/stm32f4xx_hal_cortex.c
    ${stm32cubef4_SOURCE_DIR}/Drivers/STM32F4xx_HAL_Driver/Src/stm32f4xx_hal_rcc.c
    ${stm32cubef4_SOURCE_DIR}/Drivers/STM32F4xx_HAL_Driver/Src/stm32f4xx_hal_usart.c
    ${stm32cubef4_SOURCE_DIR}/Drivers/STM32F4xx_HAL_Driver/Src/stm32f4xx_hal_gpio.c
    ${stm32cubef4_SOURCE_DIR}/Drivers/STM32F4xx_HAL_Driver/Src/stm32f4xx_hal_tim.c)
set(
    HAL_INCLUDES
    ${stm32cubef4_SOURCE_DIR}/Drivers/CMSIS/Include
    ${stm32cubef4_SOURCE_DIR}/Drivers/CMSIS/Device/ST/STM32F4xx/Include/
    ${stm32cubef4_SOURCE_DIR}/Drivers/STM32F4xx_HAL_Driver/Inc)

set(
    APP_SOURCES
    src/main.c
    src/modality.c
    src/logging.c
    src/stats.c
    src/ip.c
    src/comms.c
    src/sensor.c
    src/actuator.c
    src/led.c
    src/syscall.c
    src/hooks.c
    src/timer.c
    ${stm32cubef4_SOURCE_DIR}/Drivers/CMSIS/Device/ST/STM32F4xx/Source/Templates/system_stm32f4xx.c
    ${stm32cubef4_SOURCE_DIR}/Drivers/CMSIS/Device/ST/STM32F4xx/Source/Templates/gcc/startup_stm32f407xx.s)
set(
    APP_INCLUDES
    include
    include/config)

if(DEFINED STREAM_PORT_RTT)
    list(
        APPEND
        APP_INCLUDES
        include/config/trc_rtt)
    list(
        APPEND
        TRACING_INCLUDES
        include/config/trc_rtt)
elseif(DEFINED STREAM_PORT_TCP)
    list(
        APPEND
        APP_INCLUDES
        include/trc_tcp
        include/config/trc_tcp)
    list(
        APPEND
        APP_SOURCES
        src/trcStreamPort.c)
    list(
        APPEND
        TRACING_INCLUDES
        include/trc_tcp
        include/config/trc_tcp)
endif()

add_library(
    hal
    STATIC
    ${HAL_SOURCES})

target_include_directories(
    hal
    PRIVATE
    include/config
    PUBLIC
    ${HAL_INCLUDES})

target_compile_definitions(
    hal
    PUBLIC
    STM32F4xx
    STM32F407xx
    HSE_VALUE=8000000
    _FPU_PRESENT=1
    _FPU_USED=1
    ARM_MATH_CM4)

add_library(
    freertos
    STATIC
    ${TRACING_SOURCES}
    ${KERNEL_SOURCES})

target_include_directories(
    freertos
    PRIVATE
    include/config
    include
    ${HAL_INCLUDES}
    PUBLIC
    ${TRACING_INCLUDES}
    ${KERNEL_INCLUDES})

add_library(
    freertos_tcpip
    STATIC
    ${TCPIP_SOURCES})

target_include_directories(
    freertos_tcpip
    PRIVATE
    include/config
    include
    ${HAL_INCLUDES}
    PUBLIC
    ${TCPIP_INCLUDES})

target_compile_options(
    freertos_tcpip
    PRIVATE
    -Wno-format
    -Wno-shadow)

target_compile_definitions(
    freertos_tcpip
    PUBLIC
    HEAP4)

target_link_libraries(
    freertos_tcpip
    hal
    freertos)

add_executable(
    ${PROJECT_NAME}
    ${APP_SOURCES})

target_include_directories(
    ${PROJECT_NAME}
    PRIVATE
    include/config
    include)

target_compile_definitions(
    ${PROJECT_NAME}
    PRIVATE
    HEAP4)

target_link_libraries(
    ${PROJECT_NAME}
    hal
    freertos
    freertos_tcpip)

add_custom_target(
    emulate
    DEPENDS ${PROJECT_NAME})

add_custom_command(
    TARGET emulate
    POST_BUILD
    MAIN_DEPENDENCY ${PROJECT_NAME}
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    COMMAND renode --hide-monitor --disable-gui ${RENODE_SCRIPT}
    COMMENT "Emulating firmware with Renode")

add_custom_target(
    emulate-with-monitor
    DEPENDS ${PROJECT_NAME})

add_custom_command(
    TARGET emulate-with-monitor
    POST_BUILD
    MAIN_DEPENDENCY ${PROJECT_NAME}
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    COMMAND renode ${RENODE_SCRIPT}
    COMMENT "Emulating firmware with Renode")

print_exec_size()
elf_to_bin()
