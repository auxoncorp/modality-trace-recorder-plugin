/* LED ISR */

#include "FreeRTOS.h"
#include "task.h"
#include "timers.h"

#include "app_config.h"
#include "logging.h"
#include "led.h"

#include "stm32f4xx_hal_conf.h"

#define LED_BLINK_PERIOD_MS pdMS_TO_TICKS(1000)
#define LED_TIMER_NAME "LED"

static void led_timer_handler(TimerHandle_t timer);

static traceHandle g_isr_handle = NULL;

void led_init(void)
{
    INFO("Initializing LED ISR");

    g_isr_handle = xTraceSetISRProperties("LEDTimerISR", TIMER_PRIO);
    configASSERT(g_isr_handle != NULL);

    /* Mock an ISR using a timer */
    TimerHandle_t mock_isr_timer = xTimerCreate(LED_TIMER_NAME, LED_BLINK_PERIOD_MS, pdTRUE, NULL, led_timer_handler);
    configASSERT(mock_isr_timer != NULL);

    xTimerStart(mock_isr_timer, 0);
}

static void led_timer_handler(TimerHandle_t timer)
{
    vTraceStoreISRBegin(g_isr_handle);
    INFO("blink");
    vTraceStoreISREnd(0);
}
