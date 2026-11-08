LOCAL_PATH := $(call my-dir)

include $(CLEAR_VARS)
LOCAL_MODULE := xray_spawn
LOCAL_SRC_FILES := xray_spawn.c
LOCAL_LDLIBS := -llog
include $(BUILD_SHARED_LIBRARY)
