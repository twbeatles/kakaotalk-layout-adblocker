from kakao_adblocker.services import ReleaseService


def test_release_service_uses_releases_url(monkeypatch):
    called = {"url": ""}
    monkeypatch.setattr(
        "kakao_adblocker.services.ShellService.open_url",
        lambda url: called.__setitem__("url", url) or True,
    )
    ok = ReleaseService.open_releases_page()
    assert ok is True
    assert called["url"] == "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases"
