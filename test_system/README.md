# Test system for modality-trace-recorder-plugin

This project serves as an integration test suite for the plugins.
It's a simple FreeRTOS application running on an STM32F4 Discovery board emulated in [Renode](https://renode.readthedocs.io/en/latest/).

See the integration test [workflow](../.github/workflows/integration_tests.yml) for more details on how we use it
in this repository.

## Using the data from CI

The Integration Tests workflow produces a modality data artifact that can be downloaded for local use.

1. [Install Modality](https://docs.auxon.io/modality/installation/)
2. Navigate to the latest Integration Tests workflow run in the Actions tab
3. Download the tarball artifact (`trc_test_system_modality_data_$DATETIME.tar.gz`)
4. Extract it (assuming in `/tmp`)
5. Run `modalityd` with the extracted data-dir
  ```bash
  modalityd --license-key '<YOU_LICENSE_KEY> --data-dir /tmp/modalityd_data
  ```
6. Setup the user and workspace
  ```bash
  modality user auth-token --use $(< /tmp/modalityd_data/user_auth_token)
  modality workspace use ci-tests
  ```
