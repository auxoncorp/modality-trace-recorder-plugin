#include "FreeRTOS.h"
#include "queue.h"
#include "task.h"

#include "FreeRTOS_IP.h"
#include "FreeRTOS_Sockets.h"

#include "app_config.h"
#include "logging.h"
#include "wire_protocol.h"
#include "comms.h"

#define COMMS_QUEUE_LENGTH (8)

typedef struct
{
    int16_t adc_value;
    int16_t pwm_value;
} comms_msg_s;

static void comms_task(void* params);
static QueueHandle_t g_comms_queue = NULL;

void comms_init(void)
{
    INFO("Initializing actuator task");

    g_comms_queue = xQueueCreate(COMMS_QUEUE_LENGTH, sizeof(comms_msg_s));
    configASSERT(g_comms_queue != NULL);
    vTraceSetQueueName(g_comms_queue, "comms_queue");

    xTaskCreate(comms_task, COMMS_NAME, COMMS_STACK_SIZE, NULL, COMMS_PRIO, NULL);
}

void comms_send_actuator_state(int16_t adc_value, int16_t pwm_value)
{
    comms_msg_s msg =
    {
        .adc_value = adc_value,
        .pwm_value = pwm_value,
    };

    const BaseType_t was_posted = xQueueSend(g_comms_queue, &msg, 0 /* non-blocking */);
    if(was_posted != pdTRUE)
    {
        ERR("Failed to send actuator state");
    }
}

static comms_msg_s comms_recv(void)
{
    comms_msg_s msg;
    const BaseType_t was_recvd = xQueueReceive(g_comms_queue, &msg, portMAX_DELAY);
    configASSERT(was_recvd == pdTRUE);
    return msg;
}

static void comms_task(void* params)
{
    BaseType_t status;
    traceString ch;
    wire_msg_s wire_msg = {0};
    Socket_t socket = FREERTOS_INVALID_SOCKET;
    struct freertos_sockaddr addr = {0};
    (void) params;

    ch = xTraceRegisterString("comms_tx");

    while(FreeRTOS_IsNetworkUp() == pdFALSE)
    {
        vTaskDelay(pdMS_TO_TICKS(10));
    }

    addr.sin_port = FreeRTOS_htons(SENSOR_DATA_PORT);
    addr.sin_addr = FreeRTOS_inet_addr_quick(
            configIP_ADDR0,
            configIP_ADDR1,
            configIP_ADDR2,
            255);

    socket = FreeRTOS_socket(
            FREERTOS_AF_INET,
            FREERTOS_SOCK_DGRAM,
            FREERTOS_IPPROTO_UDP);
    configASSERT(socket != FREERTOS_INVALID_SOCKET);

    INFO("Comms network ready");

    while(1)
    {
        const comms_msg_s comms_msg = comms_recv();

        wire_msg.magic0 = WIRE_MAGIC0;
        wire_msg.magic1 = WIRE_MAGIC1;
        wire_msg.type = WIRE_TYPE_ACTUATOR_STATE;
        wire_msg.seqnum += 1;
        wire_msg.adc = comms_msg.adc_value;
        wire_msg.pwm = comms_msg.pwm_value;

        xTracePrintF(ch, "%u %u %d %d", wire_msg.type, wire_msg.seqnum, wire_msg.adc, wire_msg.pwm);

        status = FreeRTOS_sendto(socket, &wire_msg, sizeof(wire_msg), 0, &addr, sizeof(addr));
        if(status < 0)
        {
            ERR("Failed to send actuator state wire message");
        }
    }
}
