/* Keep the leased executable alive while its scripted RPC child owns stdio.
 * Both remain in the process group created by the production Pi launcher. */
#include <errno.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv) {
    char **child_argv = calloc((size_t)argc + 2, sizeof(char *));
    if (child_argv == NULL) return 70;
    child_argv[0] = FIXTURE_PYTHON;
    child_argv[1] = FIXTURE_SCRIPT;
    for (int i = 1; i < argc; i++) child_argv[i + 1] = argv[i];
    pid_t child = fork();
    if (child < 0) return 71;
    if (child == 0) {
        execv(FIXTURE_PYTHON, child_argv);
        _exit(72);
    }
    free(child_argv);
    int status;
    while (waitpid(child, &status, 0) < 0) {
        if (errno != EINTR) return 73;
    }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
