/* PWM actuator task */

#include "FreeRTOS.h"
#include "queue.h"
#include "task.h"

#include "app_config.h"
#include "logging.h"
#include "comms.h"
#include "actuator.h"

#include "stm32f4xx_hal_conf.h"

#define UPDATE_PERIOD_MS pdMS_TO_TICKS(100)
#define ADC_QUEUE_LENGTH (8)

static void actuator_task(void* params);
static QueueHandle_t g_adc_queue = NULL;

void actuator_init(void)
{
    INFO("Initializing actuator task");

    g_adc_queue = xQueueCreate(ADC_QUEUE_LENGTH, sizeof(int16_t));
    configASSERT(g_adc_queue != NULL);
    vTraceSetQueueName(g_adc_queue, "adc_queue");

    xTaskCreate(actuator_task, ACTUATOR_NAME, ACTUATOR_STACK_SIZE, NULL, ACTUATOR_PRIO, NULL);
}

void actuator_send_adc_data(int16_t adc_value)
{
    const BaseType_t was_posted = xQueueSend(g_adc_queue, &adc_value, 0 /* non-blocking */);
    if(was_posted != pdTRUE)
    {
        ERR("Failed to send ADC data");
    }
}

static int16_t adc_recv(void)
{
    int16_t adc_value;
    const BaseType_t was_recvd = xQueueReceive(g_adc_queue, &adc_value, portMAX_DELAY);
    configASSERT(was_recvd == pdTRUE);
    return adc_value;
}

static void actuator_task(void* params)
{
    int16_t pwm_value;
    traceString pwm_ch;
    (void) params;

    pwm_ch = xTraceRegisterString("pwm");

    while(1)
    {
        const int16_t adc_value = adc_recv();
        pwm_value = -1 * adc_value;
        vTracePrintF(pwm_ch, "%d", pwm_value);
        comms_send_actuator_state(adc_value, pwm_value);
    }
}
