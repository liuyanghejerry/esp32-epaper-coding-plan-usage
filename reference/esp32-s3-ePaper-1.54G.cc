#include "application.h"
#include "button.h"
#include "codecs/box_audio_codec.h"
#include "codecs/es8311_audio_codec.h"
#include "config.h"
#include "wifi_board.h"
#include <wifi_station.h>

#include "power_save_timer.h"
#include "user_app.h"
#include <driver/i2c_master.h>
#include <esp_log.h>

#include "mcp_server.h"

#define TAG "esp32-s3-ePaper-1.54G"

extern bool g_sdcard_ok;

class waveshare_s3_ePaper_1_54G : public WifiBoard {
  private:
    i2c_master_bus_handle_t codec_i2c_bus_;
    Button                  boot_button_;
    PowerSaveTimer         *power_save_timer_;

    void InitializeCodecI2c() {
        ESP_ERROR_CHECK(i2c_master_get_bus_handle(0, &codec_i2c_bus_));
    }

    //void InitializePowerSaveTimer() {
    //    power_save_timer_ = new PowerSaveTimer(-1, 60, -1);
    //    power_save_timer_->OnEnterSleepMode([this]() { //sleep
    //        ESP_LOGE("power", "Fall asleep");
    //        auto& app = Application::GetInstance();
    //        app.ToggleChatState();
    //        app.ToggleChatState();
    //        gpio_set_level((gpio_num_t) 45, 1);
    //    });
    //    power_save_timer_->OnExitSleepMode([this]() { //Exit sleep
    //        ESP_LOGE("power", "Exit sleep");
    //        gpio_set_level((gpio_num_t) 45, 0);
    //    });
    //    power_save_timer_->OnShutdownRequest([this]() { //Power off
    //        ESP_LOGE("power", "Power off");
    //        auto& app = Application::GetInstance();
    //        app.ToggleChatState();
    //        app.ToggleChatState();
    //    });
    //    power_save_timer_->SetEnabled(true); //Enable the timer
    //}

    void InitializeButtons() {
        boot_button_.OnClick([this]() {
            auto &app = Application::GetInstance();
            if (app.GetDeviceState() == kDeviceStateStarting && !WifiStation::GetInstance().IsConnected()) {
                ResetWifiConfiguration();
            }
            app.ToggleChatState();
        });
    }

    void InitializeTools() {
        auto &mcp_server = McpServer::GetInstance();
        mcp_server.AddTool("self.disp.SwitchPictures", "切换本地或 SD 卡中的图片，通过整数参数指定图片序号（如 “显示第 1 张图片”）", PropertyList({Property("value", kPropertyTypeInteger, 1, sdcard_bmp_Quantity)}), [this](const PropertyList &properties) -> ReturnValue {
            int value = properties["value"].value<int>();
            ESP_LOGE("vlaue", "%d", value);
            sdcard_doc_count = value;
            xEventGroupClearBits(ai_IMG_Score_Group, 0x01); //There is no need to poll for photos anymore.
            xEventGroupSetBits(epaper_groups, 0x02);        //  0000  0010
            return true;
        });

        mcp_server.AddTool("self.disp.getNumberimages", "获取 SD 卡中存储的图片文件总数，无输入参数，返回整数类型的图片数量", PropertyList(), [this](const PropertyList &) -> ReturnValue {
            xEventGroupSetBits(ai_IMG_Group, 0x02); //Retrieve the images from the SD card
            if (xSemaphoreTake(ai_img_while_semap, pdMS_TO_TICKS(2000)) == pdTRUE) {
                if(!g_sdcard_ok || sdcard_bmp_Quantity == 0 ) {
                    xEventGroupSetBits(epaper_groups, 0x02); 
                    return "SD卡里空空如也，一张图都没有";
                }
                return sdcard_bmp_Quantity;
            } else {
                return false;
            }
        });

        mcp_server.AddTool("self.disp.isSLeep", "使设备进入低功耗睡眠模式，关闭显示等非必要功能以节省电量，无参数，执行后设备进入休眠状态", PropertyList(), [this](const PropertyList &) -> ReturnValue {
            ESP_LOGI("MCP", "进入MCP isSLeep");
            xEventGroupSetBits(ai_IMG_Group, 0x08); //Low-power mode
            return true;
        });

        mcp_server.AddTool("self.disp.isSHTC3", "获取设备温度和湿度", PropertyList(), [this](const PropertyList &) -> ReturnValue {
            ESP_LOGI("MCP", "进入MCP isSHTC3");
            char *str = Get_TemperatureHumidity();
            if(str) return str;
            else return NULL;
        });

         mcp_server.AddTool("self.disp.isBAT", "获取设备电量", PropertyList(), [this](const PropertyList &) -> ReturnValue {
            ESP_LOGI("MCP", "进入MCP isBAT");
             return Get_Batterylevel();
        });
    }

  public:
    waveshare_s3_ePaper_1_54G()
        : boot_button_(BOOT_BUTTON_GPIO) {
        InitializeCodecI2c();
        //InitializePowerSaveTimer();
        User_xiaozhi_app_init();
        InitializeButtons();
        InitializeTools();
    }

    virtual AudioCodec *GetAudioCodec() override {
        static Es8311AudioCodec audio_codec(codec_i2c_bus_, I2C_NUM_0, AUDIO_INPUT_SAMPLE_RATE, AUDIO_OUTPUT_SAMPLE_RATE,
            AUDIO_I2S_GPIO_MCLK, AUDIO_I2S_GPIO_BCLK, AUDIO_I2S_GPIO_WS, AUDIO_I2S_GPIO_DOUT, AUDIO_I2S_GPIO_DIN,
            AUDIO_CODEC_PA_PIN, AUDIO_CODEC_ES8311_ADDR);
        return &audio_codec;
    }

    //virtual void SetPowerSaveMode(bool enabled) override {
    //    if (!enabled) {
    //        power_save_timer_->WakeUp();
    //    }
    //    WifiBoard::SetPowerSaveMode(enabled);
    //}
};

DECLARE_BOARD(waveshare_s3_ePaper_1_54G);

/*
afe_config->agc_init = false;
*/