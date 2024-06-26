#ifndef APP_CONFIG_H
#define APP_CONFIG_H

#ifdef __cplusplus
extern "C" {
#endif

#define configHOST_NAME "demo-firmware"
#define configDEVICE_NICK_NAME configHOST_NAME

#define SENSOR_NAME "Sensor"
#define SENSOR_PRIO (tskIDLE_PRIORITY + 4)
#define SENSOR_STACK_SIZE configMINIMAL_STACK_SIZE

#define ACTUATOR_NAME "Actuator"
#define ACTUATOR_PRIO (tskIDLE_PRIORITY + 4)
#define ACTUATOR_STACK_SIZE configMINIMAL_STACK_SIZE

#define COMMS_NAME "Comms"
#define COMMS_PRIO (tskIDLE_PRIORITY + 4)
#define COMMS_STACK_SIZE configMINIMAL_STACK_SIZE

#define STATS_NAME "Stats"
#define STATS_PRIO (tskIDLE_PRIORITY + 1)
#define STATS_STACK_SIZE (2 * configMINIMAL_STACK_SIZE)

#define IDLE_NAME "IDLE"
#define IDLE_PRIO tskIDLE_PRIORITY
#define IDLE_STACK_SIZE configMINIMAL_STACK_SIZE

#define TIMER_NAME "Tmr Svc"
#define TIMER_PRIO configTIMER_TASK_PRIORITY
#define TIMER_STACK_SIZE configTIMER_TASK_STACK_DEPTH

#define TZCTRL_NAME "TzCtrl"
#define TZCTRL_PRIO TRC_CFG_CTRL_TASK_PRIORITY
#define TZCTRL_STACK_SIZE TRC_CFG_CTRL_TASK_STACK_SIZE

#define IP_NAME "IP-task"
#define IP_PRIO ipconfigIP_TASK_PRIORITY
#define IP_STACK_SIZE ipconfigIP_TASK_STACK_SIZE_WORDS

#define EMAC_NAME "EMAC"
#define EMAC_PRIO nwETHERNET_RX_HANDLER_TASK_PRIORITY
#define EMAC_STACK_SIZE nwRX_TASK_STACK_SIZE

#ifdef __cplusplus
}
#endif

#endif /* APP_CONFIG_H */
