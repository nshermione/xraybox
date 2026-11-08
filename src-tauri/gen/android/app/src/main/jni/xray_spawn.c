#include <android/log.h>
#include <errno.h>
#include <fcntl.h>
#include <jni.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define LOG_TAG "xray_spawn"

static char **jstring_array(JNIEnv *env, jobjectArray array) {
  if (array == NULL) {
    return NULL;
  }
  jsize count = (*env)->GetArrayLength(env, array);
  char **out = calloc((size_t)count + 1, sizeof(char *));
  if (out == NULL) {
    return NULL;
  }
  for (jsize i = 0; i < count; i++) {
    jstring js = (jstring)(*env)->GetObjectArrayElement(env, array, i);
    if (js == NULL) {
      continue;
    }
    const char *utf = (*env)->GetStringUTFChars(env, js, NULL);
    if (utf != NULL) {
      out[i] = strdup(utf);
      (*env)->ReleaseStringUTFChars(env, js, utf);
    }
    (*env)->DeleteLocalRef(env, js);
  }
  return out;
}

static void free_string_array(char **items) {
  if (items == NULL) {
    return;
  }
  for (char **cursor = items; *cursor != NULL; cursor++) {
    free(*cursor);
  }
  free(items);
}

JNIEXPORT jint JNICALL
Java_io_github_xraybox_vpn_XrayVpnService_nativeSpawnXray(
    JNIEnv *env,
    jobject thiz,
    jstring jpath,
    jobjectArray jargs,
    jobjectArray jenvp,
    jint tun_fd,
    jstring jcwd,
    jstring jlog
) {
  (void)thiz;
  const char *path = (*env)->GetStringUTFChars(env, jpath, NULL);
  const char *cwd = jcwd == NULL ? NULL : (*env)->GetStringUTFChars(env, jcwd, NULL);
  const char *log_path = jlog == NULL ? NULL : (*env)->GetStringUTFChars(env, jlog, NULL);
  char **argv = jstring_array(env, jargs);
  char **envp = jstring_array(env, jenvp);
  if (path == NULL || argv == NULL || envp == NULL) {
    if (path != NULL) {
      (*env)->ReleaseStringUTFChars(env, jpath, path);
    }
    if (cwd != NULL) {
      (*env)->ReleaseStringUTFChars(env, jcwd, cwd);
    }
    if (log_path != NULL) {
      (*env)->ReleaseStringUTFChars(env, jlog, log_path);
    }
    free_string_array(argv);
    free_string_array(envp);
    return -1;
  }

  pid_t pid = fork();
  if (pid != 0) {
    (*env)->ReleaseStringUTFChars(env, jpath, path);
    if (cwd != NULL) {
      (*env)->ReleaseStringUTFChars(env, jcwd, cwd);
    }
    if (log_path != NULL) {
      (*env)->ReleaseStringUTFChars(env, jlog, log_path);
    }
    free_string_array(argv);
    free_string_array(envp);
    return pid < 0 ? -1 : pid;
  }

  if (cwd != NULL && cwd[0] != '\0' && chdir(cwd) != 0) {
    _exit(127);
  }
  int flags = fcntl(tun_fd, F_GETFD);
  if (flags >= 0) {
    fcntl(tun_fd, F_SETFD, flags & ~FD_CLOEXEC);
  }
  if (log_path != NULL && log_path[0] != '\0') {
    int log_fd = open(log_path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (log_fd >= 0) {
      dup2(log_fd, STDOUT_FILENO);
      dup2(log_fd, STDERR_FILENO);
      if (log_fd > STDERR_FILENO) {
        close(log_fd);
      }
    }
  }
  execve(path, argv, envp);
  __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, "execve failed: %s", strerror(errno));
  _exit(127);
}

JNIEXPORT void JNICALL
Java_io_github_xraybox_vpn_XrayVpnService_nativeKillXray(JNIEnv *env, jobject thiz, jint pid) {
  (void)env;
  (void)thiz;
  if (pid > 1) {
    kill((pid_t)pid, SIGKILL);
  }
}
