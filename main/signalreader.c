#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include <unistd.h>
#include "driver/gpio.h"
#include "driver/gptimer.h"
#include "esp_log.h"
#include "driver/uart.h"
#include "hal/uart_types.h"
#include "sdkconfig.h"
#include "esp_timer.h"
#include <stdint.h>

// Sampling rate in Hz. You need to check that it isn't big enough to
// overload the serial line!

// Maximum rated numbers @ 240 MHz:
// 100 kHz @ 128 K baud.

#define SAMPLING_RATE 100000
#define SAMPLES_GPIO_SOURCE GPIO_NUM_14
#define SAMPLES_GPIO_PULL_MODE GPIO_PULLUP_ONLY

// UART configuration
#define UART_TX_GPIO GPIO_NUM_17
#define UART_RX_GPIO GPIO_NUM_16

#define UART_PORT_NUM      (2)
#define UART_BAUD_RATE (128000)

// Other stuff
#define SAMPLING_RATE_PERIOD_US (US_IN_SECOND / SAMPLING_RATE)
#define TAG "signal_reader"
#define US_IN_SECOND 1000000

volatile uint64_t samples_sent = 0;
volatile bool io_error;
uint8_t cur_sample = 0;
uint8_t cur_sample_bits = 0;
static IRAM_ATTR bool sampler_clock_isr(gptimer_handle_t timer, const gptimer_alarm_event_data_t *edata, void *user_ctx) {
  int value = gpio_get_level(SAMPLES_GPIO_SOURCE);
  cur_sample = ((cur_sample << 1) | value);

  if (++cur_sample_bits >= 8) {
    if (uart_write_bytes(UART_PORT_NUM, &cur_sample, 1) < 0) {
      io_error = true;
    } else {
      samples_sent += 8;
    }

    cur_sample = 0;
    cur_sample_bits = 0;
  }

  return true;
}

int serial_init(void) {
  int err;

  uart_config_t uart_config = {
    .baud_rate = UART_BAUD_RATE,
    .data_bits = UART_DATA_8_BITS,
    .parity    = UART_PARITY_DISABLE,
    .stop_bits = UART_STOP_BITS_1,
    .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
    .source_clk = UART_SCLK_DEFAULT,
  };
  int intr_alloc_flags = 0;

#if CONFIG_UART_ISR_IN_IRAM
  intr_alloc_flags = ESP_INTR_FLAG_IRAM;
#endif

  if ((err = uart_driver_install(UART_PORT_NUM, 512, 0, 0, NULL, intr_alloc_flags)) != ESP_OK) {
    return err;
  }

  if ((err = uart_param_config(UART_PORT_NUM, &uart_config)) != ESP_OK) {
    return err;
  }

  if ((err = uart_set_pin(UART_PORT_NUM, UART_TX_GPIO, UART_RX_GPIO, UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE)) != ESP_OK) {
    return err;
  }

  return ESP_OK;
}

int sampler_clk_init(gptimer_handle_t* handle) {
  int err;

  gptimer_config_t timer_config = {
    .clk_src = GPTIMER_CLK_SRC_DEFAULT,
    .direction = GPTIMER_COUNT_UP,
    .resolution_hz = 1 * 1000 * 1000, // 1MHz, 1 tick = 1us
  };

  gptimer_alarm_config_t alarm_config = {
    .reload_count = 0, // counter will reload with 0 on alarm event
    .alarm_count = SAMPLING_RATE_PERIOD_US, // Sample period in us is = to alarm count because clk is set to 1 MHz
    .flags.auto_reload_on_alarm = true,
  };

  gptimer_event_callbacks_t callback = {
    .on_alarm = sampler_clock_isr
  };

  if ((err = gptimer_new_timer(&timer_config, handle)) != ESP_OK) {
    return err;
  }

  if ((err = gptimer_set_alarm_action(*handle, &alarm_config)) != ESP_OK) {
    return err;
  }

  if ((err = gptimer_register_event_callbacks(*handle, &callback, NULL)) != ESP_OK) {
    return err;
  }

  if ((err = gptimer_enable(*handle)) != ESP_OK) {
    return err;
  }

  return ESP_OK;
}

esp_timer_handle_t sampler_timer;
void app_main(void) {
  gptimer_handle_t sampler;
  int64_t clk_begin_time;
  int64_t clk_elapsed;
  uint64_t expected_samples;

  ESP_LOGI(TAG, "Starting...");
  gpio_reset_pin(SAMPLES_GPIO_SOURCE);
  gpio_set_direction(SAMPLES_GPIO_SOURCE, GPIO_MODE_INPUT);
  gpio_set_pull_mode(SAMPLES_GPIO_SOURCE, SAMPLES_GPIO_PULL_MODE);

  ESP_ERROR_CHECK(serial_init());
  ESP_ERROR_CHECK(sampler_clk_init(&sampler));

  clk_begin_time = esp_timer_get_time();
  ESP_ERROR_CHECK(gptimer_start(sampler));

  ESP_LOGI(TAG, "Everything initiated!");
  while (1) {
    clk_elapsed = esp_timer_get_time() - clk_begin_time;
    expected_samples = (clk_elapsed * SAMPLING_RATE) / US_IN_SECOND;

    if (io_error) {
      io_error = false;
      ESP_LOGE(TAG, "FATAL! I/O ERROR");
    }

    if (expected_samples < samples_sent) {
      ESP_LOGE(TAG, "Can't keep up!. Reduce signal sampling rate or increase serial port baud rate!");
    }

    ESP_LOGI(TAG, "Record duration: %llu second(s); Samples sent: %llu", (samples_sent / SAMPLING_RATE), samples_sent);
    vTaskDelay(500 / portTICK_PERIOD_MS);
  }
}
