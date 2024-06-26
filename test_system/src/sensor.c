/* ADC sensor task */

#include "FreeRTOS.h"
#include "task.h"

#include "app_config.h"
#include "logging.h"
#include "actuator.h"
#include "sensor.h"

#include "stm32f4xx_hal_conf.h"

#define UPDATE_PERIOD_MS pdMS_TO_TICKS(100)

static void sensor_task(void* params);
static const uint8_t SINE_WAVE[256];

void sensor_init(void)
{
    INFO("Initializing sensor task");
    xTaskCreate(sensor_task, SENSOR_NAME, SENSOR_STACK_SIZE, NULL, SENSOR_PRIO, NULL);
}

static void sensor_task(void* params)
{
    int16_t adc_value;
    int i;
    TickType_t next_wake;
    traceResult tr;
    traceString ch;
    TraceStateMachineHandle_t sm;
    TraceStateMachineStateHandle_t init_state;
    TraceStateMachineStateHandle_t reading_state;
    TraceStateMachineStateHandle_t suspended_state;
    (void) params;

    tr = xTraceStringRegister("adc", &ch);
    configASSERT(tr == TRC_SUCCESS);

    tr = xTraceStateMachineCreate("sensor_sm", &sm);
    configASSERT(tr == TRC_SUCCESS);
    tr = xTraceStateMachineStateCreate(sm, "INIT", &init_state);
    configASSERT(tr == TRC_SUCCESS);
    tr = xTraceStateMachineStateCreate(sm, "READING", &reading_state);
    configASSERT(tr == TRC_SUCCESS);
    tr = xTraceStateMachineStateCreate(sm, "SUSPENDED", &suspended_state);
    configASSERT(tr == TRC_SUCCESS);

    tr = xTraceStateMachineSetState(sm, init_state);
    configASSERT(tr == TRC_SUCCESS);
    tr = xTraceStateMachineSetState(sm, reading_state);
    configASSERT(tr == TRC_SUCCESS);

    i = 0;
    next_wake = xTaskGetTickCount();
    while(1)
    {
        const BaseType_t was_delayed = xTaskDelayUntil(&next_wake, UPDATE_PERIOD_MS);
        if(was_delayed == pdFALSE)
        {
            WARN("Sensor deadline missed");
        }

        adc_value = (int16_t) ((int8_t) SINE_WAVE[i]);

        tr = xTracePrintF(ch, "%d", adc_value);
        configASSERT(tr == TRC_SUCCESS);

        actuator_send_adc_data(adc_value);
        i += 1;

        /* Shutdown the task */
        if(i == 256)
        {
            tr = xTraceStateMachineSetState(sm, suspended_state);
            configASSERT(tr == TRC_SUCCESS);
            WARN("Calling vTaskSuspend");
            vTaskSuspend(NULL);
        }
    }
}

/* http://aquaticus.info/pwm-sine-wave */
static const uint8_t SINE_WAVE[256] =
{
    0x80, 0x83, 0x86, 0x89, 0x8C, 0x90, 0x93, 0x96,
    0x99, 0x9C, 0x9F, 0xA2, 0xA5, 0xA8, 0xAB, 0xAE,
    0xB1, 0xB3, 0xB6, 0xB9, 0xBC, 0xBF, 0xC1, 0xC4,
    0xC7, 0xC9, 0xCC, 0xCE, 0xD1, 0xD3, 0xD5, 0xD8,
    0xDA, 0xDC, 0xDE, 0xE0, 0xE2, 0xE4, 0xE6, 0xE8,
    0xEA, 0xEB, 0xED, 0xEF, 0xF0, 0xF1, 0xF3, 0xF4,
    0xF5, 0xF6, 0xF8, 0xF9, 0xFA, 0xFA, 0xFB, 0xFC,
    0xFD, 0xFD, 0xFE, 0xFE, 0xFE, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFE, 0xFE, 0xFD,
    0xFD, 0xFC, 0xFB, 0xFA, 0xFA, 0xF9, 0xF8, 0xF6,
    0xF5, 0xF4, 0xF3, 0xF1, 0xF0, 0xEF, 0xED, 0xEB,
    0xEA, 0xE8, 0xE6, 0xE4, 0xE2, 0xE0, 0xDE, 0xDC,
    0xDA, 0xD8, 0xD5, 0xD3, 0xD1, 0xCE, 0xCC, 0xC9,
    0xC7, 0xC4, 0xC1, 0xBF, 0xBC, 0xB9, 0xB6, 0xB3,
    0xB1, 0xAE, 0xAB, 0xA8, 0xA5, 0xA2, 0x9F, 0x9C,
    0x99, 0x96, 0x93, 0x90, 0x8C, 0x89, 0x86, 0x83,
    0x80, 0x7D, 0x7A, 0x77, 0x74, 0x70, 0x6D, 0x6A,
    0x67, 0x64, 0x61, 0x5E, 0x5B, 0x58, 0x55, 0x52,
    0x4F, 0x4D, 0x4A, 0x47, 0x44, 0x41, 0x3F, 0x3C,
    0x39, 0x37, 0x34, 0x32, 0x2F, 0x2D, 0x2B, 0x28,
    0x26, 0x24, 0x22, 0x20, 0x1E, 0x1C, 0x1A, 0x18,
    0x16, 0x15, 0x13, 0x11, 0x10, 0x0F, 0x0D, 0x0C,
    0x0B, 0x0A, 0x08, 0x07, 0x06, 0x06, 0x05, 0x04,
    0x03, 0x03, 0x02, 0x02, 0x02, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x02, 0x02, 0x02, 0x03,
    0x03, 0x04, 0x05, 0x06, 0x06, 0x07, 0x08, 0x0A,
    0x0B, 0x0C, 0x0D, 0x0F, 0x10, 0x11, 0x13, 0x15,
    0x16, 0x18, 0x1A, 0x1C, 0x1E, 0x20, 0x22, 0x24,
    0x26, 0x28, 0x2B, 0x2D, 0x2F, 0x32, 0x34, 0x37,
    0x39, 0x3C, 0x3F, 0x41, 0x44, 0x47, 0x4A, 0x4D,
    0x4F, 0x52, 0x55, 0x58, 0x5B, 0x5E, 0x61, 0x64,
    0x67, 0x6A, 0x6D, 0x70, 0x74, 0x77, 0x7A, 0x7D
};
