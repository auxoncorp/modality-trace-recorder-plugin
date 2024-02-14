#ifndef COMMS_H
#define COMMS_H

#ifdef __cplusplus
extern "C" {
#endif

void comms_init(void);

void comms_send_actuator_state(int16_t adc_value, int16_t pwm_value);

#ifdef __cplusplus
}
#endif

#endif /* COMMS_H */
